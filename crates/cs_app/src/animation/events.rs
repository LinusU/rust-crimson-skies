//! The measured event grammar of one animation record's sequence block
//! (task #690, `F20-EVENT-GRAMMAR`).
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`,
//! non-negotiable behavior 2 ("Animation unknowns must retain source locator
//! and block affected gameplay transitions. Do not assume MechWarrior event
//! semantics apply to CS.").
//!
//! # What this module measures
//!
//! [`cs_formats::zbd`] walks an animation record's sequence blocks and hands
//! each block's **raw event bytes** over untouched. This module reads the
//! grammar those bytes hold, and nothing more:
//!
//! 1. **One event is a header plus its own payload.** The header is eight
//!    bytes: a tag word (`u32`) then a length word (`u32`). The tag's low byte
//!    is the opcode; its second byte takes the measured values `1`, `2` and
//!    `3`; its high half is always zero. The length is the event's **own byte
//!    length including the header**, so stepping by it consumes the stream
//!    exactly. [`walk_event_stream`] reproduces every one of the 56 994 retail
//!    blocks (242 391 events, 16 132 948 bytes) with no byte left over and no
//!    malformed tag — that check lives in the acceptance tests, not here.
//! 2. **Each opcode is one authored statement.** A declaration's
//!    `SEQUENCE_DEFINITION` statements, in stored order, line up one-for-one
//!    with a bound record's ordinary sequence events once the declaration's
//!    leading `ACTIVATION` statement (which the payload does not store as an
//!    event) is dropped. Measured over 477 declaration/record pairs and 1 440
//!    sequences: **zero** opcode/statement conflicts. So an opcode's name is
//!    the installation's own spelling of that statement
//!    (`OBJECT_MOTION_FROM_TO`, `CALL_ANIMATION`, …), never a name borrowed
//!    from the MechWarrior 3 grammar.
//! 3. **Two timing fields are measured.** Payload word `0` is the statement's
//!    `START_TIME`: joined statements of ten opcodes state the value and word
//!    `0` equals it every time, no joined statement without a `START_TIME` has
//!    a nonzero word `0`, and across all 56 994 blocks word `0` is nonzero
//!    exactly for the opcodes whose statement vocabulary can carry
//!    `START_TIME`. The payload's **last** word is the statement's `RUN_TIME`,
//!    measured for the opcodes [`OpcodeInfo::run_time`] marks
//!    [`RunTimeEvidence::ValueMatch`] — joined statements, zero mismatches.
//!
//! # What this module does **not** measure
//!
//! * **Opcodes 13, 17, 26 and 28 join no statement** (496 retail events). They
//!   are [`EventClass::Unknown`], their own claim travels with them
//!   ([`OPCODE_NOT_MEASURED_CLAIM`]) and a record carrying one is refused.
//! * **Opcode 5's `RUN_TIME` position is not value-matched.** Its statement
//!   kind states `RUN_TIME` in the installation's declarations, so a duration
//!   for such a record could only be guessed: it is refused under
//!   [`RUN_TIME_NOT_MEASURED_CLAIM`] instead.
//! * **Payload bodies beyond those two timing fields stay raw.** A name field,
//!   a node index or a colour is not read here, so no transform, no unit and
//!   no pose is recovered from an event.
//! * **Nothing says what the 2000 engine did** with any of it. `retail` is
//!   file access, not an original run: every measurement here is
//!   [`ClaimStatus::ObservedTool`] and never `verified_original`.

use std::fmt;

use cs_types::evidence::{ClaimId, ClaimStatus};

use RunTimeEvidence as Rt;
use StartTimeEvidence as St;

/// Bytes of one event's header: the tag word, then the length word.
pub const EVENT_HEADER_BYTES: usize = 8;

/// The claim the measured event grammar is recorded under.
pub const EVENT_GRAMMAR_CLAIM: &str = "f20-anim.sequence-event-grammar-measured";

/// The claim an opcode whose statement never joins carries.
pub const OPCODE_NOT_MEASURED_CLAIM: &str = "f20-anim.event-opcode-not-measured";

/// The claim a record whose `RUN_TIME` position is unmeasured carries.
pub const RUN_TIME_NOT_MEASURED_CLAIM: &str = "f20-anim.event-run-time-position-unmeasured";

/// The claim a stream that does not walk carries.
pub const EVENT_STREAM_NOT_DECODED_CLAIM: &str = "f20-anim.sequence-event-stream-not-decoded";

/// Why a record containing an unmeasured opcode is refused.
pub const OPCODE_NOT_MEASURED_REASON: &str = "the block carries an opcode no declaration of the installation joins to a statement, so its \
     class, its timing and its effect are unmeasured and nothing may be played from it";

/// Why a record whose `RUN_TIME` position is unmeasured is refused.
pub const RUN_TIME_NOT_MEASURED_REASON: &str = "the block carries an opcode whose statement states a RUN_TIME whose position in the payload \
     has not been value-matched, so the record's duration could only be guessed";

/// The evidence class of every measurement in this module.
#[must_use]
pub const fn event_evidence() -> ClaimStatus {
    ClaimStatus::ObservedTool
}

/// The claim id of the measured grammar, as a validated [`ClaimId`].
///
/// # Panics
///
/// Panics only on a malformed module constant, which is an authoring error.
#[must_use]
pub fn grammar_claim() -> ClaimId {
    ClaimId::new(EVENT_GRAMMAR_CLAIM).expect("the event-grammar claim id is a static constant")
}

/// How the original's own statement vocabulary classifies one opcode.
///
/// The **classes are this project's grouping** of the installation's statement
/// spellings: the spellings are measured (the join in the module documentation)
/// and the grouping is a report dimension. Neither claims to state what the
/// original's engine did when it executed a statement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EventClass {
    /// A statement that moves, rotates, scales or fades something over time.
    Motion,
    /// A statement that selects a state without moving anything.
    State,
    /// A statement that fires a callback or a weapon: the gameplay marker.
    Marker,
    /// A statement about a sound.
    Sound,
    /// A statement that calls, stops, loops or branches.
    Control,
    /// No statement joins this opcode, so nothing is classed.
    Unknown,
}

impl EventClass {
    /// Every class in report order.
    pub const ALL: [Self; 6] = [
        Self::Motion,
        Self::State,
        Self::Marker,
        Self::Sound,
        Self::Control,
        Self::Unknown,
    ];

    /// The stable label a report groups by.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Motion => "motion",
            Self::State => "state",
            Self::Marker => "marker",
            Self::Sound => "sound",
            Self::Control => "control",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for EventClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// How this module knows that payload word `0` holds a statement's
/// `START_TIME`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StartTimeEvidence {
    /// Joined statements of this opcode state a `START_TIME` and word `0`
    /// equals it every time.
    ValueMatch,
    /// The opcode's statement kind states `START_TIME` in the installation's
    /// declarations, and word `0` is nonzero in the corpus for this opcode only
    /// where such a statement can appear (never value-matched here).
    Vocabulary,
    /// Word `0` is zero in **every** retail event of this opcode (measured
    /// over all 56 994 blocks), so the measured start is zero whatever the
    /// field is. For 12 of the 13 `Absent` opcodes the statement kind also
    /// never states `START_TIME`; opcode 47 shares `OBJECT_MOTION_SI_SCRIPT`
    /// with opcode 12, whose statements do, and still stores only zeros here.
    Absent,
    /// The opcode joins no statement, so nothing is established. A record
    /// carrying it is refused before timing is read.
    Unmeasured,
}

impl StartTimeEvidence {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::ValueMatch => "value_match",
            Self::Vocabulary => "vocabulary",
            Self::Absent => "absent",
            Self::Unmeasured => "unmeasured",
        }
    }
}

/// How this module knows that the payload's **last** word holds a statement's
/// `RUN_TIME`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RunTimeEvidence {
    /// Joined statements of this opcode state a `RUN_TIME` and the last word
    /// equals it every time.
    ValueMatch,
    /// The opcode's statement kind states `RUN_TIME` in the installation's
    /// declarations, but no joined statement fixes the position. A record
    /// carrying such an event is refused rather than given a short duration.
    Unmatched,
    /// The opcode's statement kind never states `RUN_TIME`, so the statement
    /// holds none and contributes zero to the record's duration.
    Absent,
    /// The opcode joins no statement, so nothing is established. A record
    /// carrying it is refused before timing is read.
    Unmeasured,
}

impl RunTimeEvidence {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::ValueMatch => "value_match",
            Self::Unmatched => "unmatched",
            Self::Absent => "absent",
            Self::Unmeasured => "unmeasured",
        }
    }
}

/// What one stored opcode is, as far as the installation states it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpcodeInfo {
    /// The opcode byte (the tag word's low byte).
    pub opcode: u8,
    /// The installation's own statement spelling for this opcode, verbatim.
    /// Empty only for an opcode no declaration joins.
    pub statement: &'static str,
    /// This project's class of that spelling.
    pub class: EventClass,
    /// How `START_TIME` was measured for this opcode.
    pub start_time: StartTimeEvidence,
    /// How `RUN_TIME` was measured for this opcode.
    pub run_time: RunTimeEvidence,
}

impl OpcodeInfo {
    /// Whether a record containing this opcode may report a duration.
    ///
    /// `false` exactly when one of the two timing fields is unmeasured for it,
    /// which is the two refusals named in the module documentation.
    #[must_use]
    pub const fn timing_measured(self) -> bool {
        !matches!(self.start_time, StartTimeEvidence::Unmeasured)
            && !matches!(self.run_time, RunTimeEvidence::Unmatched)
    }

    /// The claim a record carrying this opcode is refused under, when its
    /// timing is not measured.
    #[must_use]
    pub const fn timing_gap_claim(self) -> Option<&'static str> {
        match (self.start_time, self.run_time) {
            (StartTimeEvidence::Unmeasured, _) => Some(OPCODE_NOT_MEASURED_CLAIM),
            (_, RunTimeEvidence::Unmatched) => Some(RUN_TIME_NOT_MEASURED_CLAIM),
            _ => None,
        }
    }

    /// The reason a record carrying this opcode is refused, when its timing is
    /// not measured.
    #[must_use]
    pub const fn timing_gap_reason(self) -> Option<&'static str> {
        match (self.start_time, self.run_time) {
            (StartTimeEvidence::Unmeasured, _) => Some(OPCODE_NOT_MEASURED_REASON),
            (_, RunTimeEvidence::Unmatched) => Some(RUN_TIME_NOT_MEASURED_REASON),
            _ => None,
        }
    }
}

/// Builds one measured opcode entry.
const fn entry(
    opcode: u8,
    statement: &'static str,
    class: EventClass,
    start_time: StartTimeEvidence,
    run_time: RunTimeEvidence,
) -> OpcodeInfo {
    OpcodeInfo {
        opcode,
        statement,
        class,
        start_time,
        run_time,
    }
}

/// Builds an entry for an opcode that joins no statement.
const fn unjoined(opcode: u8) -> OpcodeInfo {
    OpcodeInfo {
        opcode,
        statement: "",
        class: EventClass::Unknown,
        start_time: StartTimeEvidence::Unmeasured,
        run_time: RunTimeEvidence::Unmeasured,
    }
}

/// Every opcode the installation's retail carriers store, in one table.
///
/// Thirty-one entries join a statement; the four at the end (13, 17, 26, 28)
/// are stored by the corpus but join none, so they keep
/// [`EventClass::Unknown`] and their own claim. A census over this table sees
/// exactly what the files hold — no opcode is dropped to make a count tidy.
pub const STORED_OPCODES: [OpcodeInfo; 35] = [
    entry(1, "SOUND", EventClass::Sound, St::ValueMatch, Rt::Absent),
    entry(2, "SOUND_NODE", EventClass::Sound, St::Absent, Rt::Absent),
    entry(
        4,
        "LIGHT_STATE",
        EventClass::State,
        St::Vocabulary,
        Rt::Absent,
    ),
    entry(
        5,
        "LIGHT_ANIMATION",
        EventClass::State,
        St::Vocabulary,
        Rt::Unmatched,
    ),
    entry(
        6,
        "OBJECT_ACTIVE_STATE",
        EventClass::State,
        St::ValueMatch,
        Rt::Absent,
    ),
    entry(
        7,
        "OBJECT_TRANSLATE_STATE",
        EventClass::Motion,
        St::Absent,
        Rt::Absent,
    ),
    entry(
        8,
        "OBJECT_SCALE_STATE",
        EventClass::Motion,
        St::Absent,
        Rt::Absent,
    ),
    entry(
        9,
        "OBJECT_ROTATE_STATE",
        EventClass::Motion,
        St::Vocabulary,
        Rt::Absent,
    ),
    entry(
        10,
        "OBJECT_MOTION",
        EventClass::Motion,
        St::ValueMatch,
        Rt::ValueMatch,
    ),
    entry(
        11,
        "OBJECT_MOTION_FROM_TO",
        EventClass::Motion,
        St::ValueMatch,
        Rt::ValueMatch,
    ),
    entry(
        12,
        "OBJECT_MOTION_SI_SCRIPT",
        EventClass::Motion,
        St::ValueMatch,
        Rt::Absent,
    ),
    entry(
        14,
        "OBJECT_OPACITY_FROM_TO",
        EventClass::Motion,
        St::ValueMatch,
        Rt::ValueMatch,
    ),
    entry(
        15,
        "OBJECT_ADD_CHILD",
        EventClass::State,
        St::Absent,
        Rt::Absent,
    ),
    entry(
        16,
        "OBJECT_DELETE_CHILD",
        EventClass::State,
        St::Absent,
        Rt::Absent,
    ),
    entry(
        20,
        "CAMERA_STATE",
        EventClass::State,
        St::Absent,
        Rt::Absent,
    ),
    entry(
        22,
        "CALL_SEQUENCE",
        EventClass::Control,
        St::Vocabulary,
        Rt::Absent,
    ),
    entry(
        23,
        "STOP_SEQUENCE",
        EventClass::Control,
        St::Vocabulary,
        Rt::Absent,
    ),
    entry(
        24,
        "CALL_ANIMATION",
        EventClass::Control,
        St::ValueMatch,
        Rt::Absent,
    ),
    entry(
        25,
        "STOP_ANIMATION",
        EventClass::Control,
        St::Vocabulary,
        Rt::Absent,
    ),
    entry(
        27,
        "INVALIDATE_ANIMATION",
        EventClass::Control,
        St::ValueMatch,
        Rt::Absent,
    ),
    entry(30, "LOOP", EventClass::Control, St::ValueMatch, Rt::Absent),
    entry(31, "IF", EventClass::Control, St::Absent, Rt::Absent),
    entry(32, "ELSE", EventClass::Control, St::Absent, Rt::Absent),
    entry(33, "ELSEIF", EventClass::Control, St::Absent, Rt::Absent),
    entry(34, "ENDIF", EventClass::Control, St::Absent, Rt::Absent),
    entry(
        35,
        "CALLBACK",
        EventClass::Marker,
        St::Vocabulary,
        Rt::Absent,
    ),
    entry(
        36,
        "FBFX_COLOR_FROM_TO",
        EventClass::State,
        St::ValueMatch,
        Rt::ValueMatch,
    ),
    entry(
        41,
        "DETONATE_WEAPON",
        EventClass::Marker,
        St::Absent,
        Rt::Absent,
    ),
    entry(
        42,
        "PUFFER_STATE",
        EventClass::State,
        St::Vocabulary,
        Rt::Absent,
    ),
    entry(
        46,
        "SOUND_ADJUST",
        EventClass::Sound,
        St::Absent,
        Rt::ValueMatch,
    ),
    entry(
        47,
        "OBJECT_MOTION_SI_SCRIPT",
        EventClass::Motion,
        St::Absent,
        Rt::Absent,
    ),
    unjoined(13),
    unjoined(17),
    unjoined(26),
    unjoined(28),
];

/// The measured record of one stored opcode, or `None` for a code no retail
/// carrier stores (a record that used one would be refused by
/// [`walk_event_stream`]'s tag check rather than classed).
#[must_use]
pub const fn opcode_info(op: u8) -> Option<OpcodeInfo> {
    let mut index = 0;
    while index < STORED_OPCODES.len() {
        let info = STORED_OPCODES[index];
        if info.opcode == op {
            return Some(info);
        }
        index += 1;
    }
    None
}

/// One event header, read from a stream but not interpreted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawEvent {
    /// Where the event starts, counted from the start of the stream.
    pub offset: u64,
    /// The tag word, verbatim.
    pub tag: u32,
    /// The tag's low byte: the opcode.
    pub opcode: u8,
    /// The tag's second byte (`1`, `2` or `3` in every retail event). Its
    /// meaning is unmeasured, so it travels instead of being interpreted.
    pub group: u8,
    /// The event's own byte length, header included.
    pub length: u32,
}

impl RawEvent {
    /// Where the payload starts (the event's offset plus its header).
    pub const fn payload_offset(&self) -> u64 {
        self.offset + EVENT_HEADER_BYTES as u64
    }

    /// The payload's byte length.
    pub const fn payload_len(&self) -> u32 {
        self.length - EVENT_HEADER_BYTES as u32
    }
}

/// Why a block's bytes are not an event stream this module can walk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventStreamError {
    /// The stream ended inside an event header.
    Truncated {
        /// Where the incomplete header starts.
        offset: u64,
        /// How many bytes were left.
        remaining: usize,
    },
    /// A length word is smaller than the header or not a multiple of four.
    BadLength {
        /// Where the event starts.
        offset: u64,
        /// The stored length.
        length: u32,
    },
    /// The tag word is not the measured shape (opcode low, `1..=3` second,
    /// zero above).
    BadTag {
        /// Where the event starts.
        offset: u64,
        /// The stored tag word.
        tag: u32,
    },
    /// The stream ends before its last event's payload does.
    ShortStream {
        /// Where the event starts.
        offset: u64,
        /// The length the event claims.
        length: u32,
        /// How many bytes were actually left.
        remaining: usize,
    },
    /// The opcode joins no statement of the installation.
    UnknownOpcode {
        /// Where the event starts.
        offset: u64,
        /// The stored opcode.
        opcode: u8,
    },
}

impl EventStreamError {
    /// The stable refusal code, for callers that match rather than read.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Truncated { .. } => "truncated_header",
            Self::BadLength { .. } => "bad_length",
            Self::BadTag { .. } => "bad_tag",
            Self::ShortStream { .. } => "short_stream",
            Self::UnknownOpcode { .. } => "unknown_opcode",
        }
    }

    /// The byte offset the refusal was found at, so it keeps its source
    /// locator.
    #[must_use]
    pub const fn offset(&self) -> u64 {
        match self {
            Self::Truncated { offset, .. }
            | Self::BadLength { offset, .. }
            | Self::BadTag { offset, .. }
            | Self::ShortStream { offset, .. }
            | Self::UnknownOpcode { offset, .. } => *offset,
        }
    }

    /// The opcode the refusal names, when it is an unjoined opcode.
    #[must_use]
    pub const fn opcode(&self) -> Option<u8> {
        match self {
            Self::UnknownOpcode { opcode, .. } => Some(*opcode),
            _ => None,
        }
    }

    /// The claim the refusal is recorded under: a structural failure keeps the
    /// event-stream claim, an unjoined opcode keeps its own.
    #[must_use]
    pub const fn claim_id(&self) -> &'static str {
        match self {
            Self::UnknownOpcode { .. } => OPCODE_NOT_MEASURED_CLAIM,
            _ => EVENT_STREAM_NOT_DECODED_CLAIM,
        }
    }

    /// Why the stream was refused, in the words of the finding.
    #[must_use]
    pub const fn reason(&self) -> &'static str {
        match self {
            Self::Truncated { .. } => {
                "the event stream ends inside an event header, so the block's stored length and \
                 its bytes disagree"
            }
            Self::BadLength { .. } => {
                "an event's length word is smaller than the eight-byte header or is not a multiple \
                 of four, so it cannot step to the next event"
            }
            Self::BadTag { .. } => {
                "an event's tag word carries bits outside its measured shape (opcode in the low \
                 byte, 1..=3 in the second byte, zero above), so it is not read"
            }
            Self::ShortStream { .. } => {
                "an event claims more bytes than the block holds, so the walk would read past the \
                 stored stream"
            }
            Self::UnknownOpcode { .. } => OPCODE_NOT_MEASURED_REASON,
        }
    }
}

impl fmt::Display for EventStreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at offset {}", self.code(), self.offset())
    }
}

impl std::error::Error for EventStreamError {}

/// Walks one block's bytes into event headers, interpreting nothing.
///
/// The rule is exactly two words per event: a tag word in the measured shape,
/// and a length word that is the event's own size including the header.
/// Stepping by the length lands on the next header and consumes the stream
/// exactly.
///
/// # Errors
///
/// [`EventStreamError`] for every way the bytes are not the measured shape;
/// the offset in the error is the event's own, so a refusal keeps its source
/// locator.
pub fn walk_event_stream(events: &[u8]) -> Result<Vec<RawEvent>, EventStreamError> {
    let mut found = Vec::new();
    let mut offset = 0_usize;
    while offset < events.len() {
        let remaining = events.len() - offset;
        if remaining < EVENT_HEADER_BYTES {
            return Err(EventStreamError::Truncated {
                offset: offset as u64,
                remaining,
            });
        }
        let tag = u32::from_le_bytes(events[offset..offset + 4].try_into().expect("four bytes"));
        let length = u32::from_le_bytes(
            events[offset + 4..offset + 8]
                .try_into()
                .expect("four bytes"),
        );
        let opcode = u8::try_from(tag & 0xff).expect("a byte");
        let group = u8::try_from((tag >> 8) & 0xff).expect("a byte");
        if tag >> 16 != 0 || !(1..=3).contains(&group) {
            return Err(EventStreamError::BadTag {
                offset: offset as u64,
                tag,
            });
        }
        if length < EVENT_HEADER_BYTES as u32 || length % 4 != 0 {
            return Err(EventStreamError::BadLength {
                offset: offset as u64,
                length,
            });
        }
        let end = offset + length as usize;
        if end > events.len() {
            return Err(EventStreamError::ShortStream {
                offset: offset as u64,
                length,
                remaining,
            });
        }
        found.push(RawEvent {
            offset: offset as u64,
            tag,
            opcode,
            group,
            length,
        });
        offset = end;
    }
    Ok(found)
}

/// One event, decoded as far as the installation measures it.
#[derive(Clone, Debug, PartialEq)]
pub struct DecodedEvent {
    /// Where the event starts, counted from the start of the block's stream.
    pub offset: u64,
    /// The opcode byte.
    pub opcode: u8,
    /// The tag's second byte: `1`, `2` or `3`, meaning unmeasured.
    pub group: u8,
    /// The event's own byte length, header included.
    pub length: u32,
    /// The installation's own statement spelling for this opcode.
    pub statement: &'static str,
    /// This project's class of that spelling.
    pub class: EventClass,
    /// The statement's `START_TIME`, in the original's stored time unit (the
    /// unit itself is unmeasured).
    pub start_time: f32,
    /// The statement's `RUN_TIME` when the opcode's position is value-matched.
    /// `None` when the statement's vocabulary states no run time — it then
    /// contributes zero — or when the position is a named gap, which refuses
    /// the record before a duration is read.
    pub run_time: Option<f32>,
}

impl DecodedEvent {
    /// When the statement starts, in the original's stored time unit.
    #[must_use]
    pub const fn start_time(&self) -> f32 {
        self.start_time
    }

    /// How long the statement runs, in the original's stored time unit, when
    /// the opcode's `RUN_TIME` position is measured.
    #[must_use]
    pub const fn run_time(&self) -> Option<f32> {
        self.run_time
    }

    /// When the statement has finished: `start_time + run_time`, or
    /// `start_time` when it states no run time.
    #[must_use]
    pub fn end_time(&self) -> f32 {
        self.start_time + self.run_time.unwrap_or(0.0)
    }

    /// The opcode's measured record, or `None` for a code no carrier stores.
    #[must_use]
    pub const fn info(&self) -> Option<OpcodeInfo> {
        opcode_info(self.opcode)
    }
}

/// Reads one walked event's statement and timing out of the block's bytes.
fn read_event(events: &[u8], raw: RawEvent) -> Result<DecodedEvent, EventStreamError> {
    let info = opcode_info(raw.opcode).ok_or(EventStreamError::UnknownOpcode {
        offset: raw.offset,
        opcode: raw.opcode,
    })?;
    // The table also carries the four opcodes the corpus stores but no
    // declaration joins: they are in it so a census sees them, and refused
    // here so a record carrying one is never played.
    if matches!(info.class, EventClass::Unknown) {
        return Err(EventStreamError::UnknownOpcode {
            offset: raw.offset,
            opcode: raw.opcode,
        });
    }
    let start = raw.payload_offset() as usize;
    let end = (raw.offset + u64::from(raw.length)) as usize;
    let payload = events
        .get(start..end)
        .ok_or(EventStreamError::ShortStream {
            offset: raw.offset,
            length: raw.length,
            remaining: events.len().saturating_sub(start),
        })?;
    let word = |index: usize| -> Option<f32> {
        let bytes = payload.get(index * 4..index * 4 + 4)?;
        Some(f32::from_le_bytes(bytes.try_into().expect("four bytes")))
    };
    // Word 0 is `START_TIME` for every opcode whose evidence is not
    // `Unmeasured`; for an `Absent` opcode the measured corpus value is zero
    // either way, so the same word is read and reported.
    let start_time = if matches!(info.start_time, StartTimeEvidence::Unmeasured) {
        0.0
    } else {
        word(0).unwrap_or(0.0)
    };
    let run_time = match info.run_time {
        RunTimeEvidence::ValueMatch => {
            let words = payload.len() / 4;
            words.checked_sub(1).and_then(word).or(Some(0.0))
        }
        // The statement's vocabulary states no run time, so it holds none and
        // the record contributes zero (`end_time` reads `None` as zero).
        RunTimeEvidence::Absent => None,
        // Named gaps: the caller refuses the record (see
        // `OpcodeInfo::timing_gap_claim`) before any duration is read.
        RunTimeEvidence::Unmatched | RunTimeEvidence::Unmeasured => None,
    };
    Ok(DecodedEvent {
        offset: raw.offset,
        opcode: raw.opcode,
        group: raw.group,
        length: raw.length,
        statement: info.statement,
        class: info.class,
        start_time,
        run_time,
    })
}

/// Walks one block's bytes and decodes every event.
///
/// # Errors
///
/// [`EventStreamError`] when the bytes are not the measured shape **or** when
/// an opcode joins no statement: an undecoded opcode refuses the whole block,
/// so partial decoding can never become a partial playback.
pub fn decode_event_stream(events: &[u8]) -> Result<Vec<DecodedEvent>, EventStreamError> {
    let raw = walk_event_stream(events)?;
    raw.into_iter()
        .map(|event| read_event(events, event))
        .collect()
}

/// The duration of a decoded block: the largest `end_time` over its events,
/// in the original's stored time unit.
///
/// # Errors
///
/// [`EventStreamError`] when the stream does not walk, and the unjoined-opcode
/// refusal when an opcode joins no statement.
pub fn sequence_duration(events: &[u8]) -> Result<f32, EventStreamError> {
    let decoded = decode_event_stream(events)?;
    Ok(decoded
        .iter()
        .map(DecodedEvent::end_time)
        .fold(0.0_f32, f32::max))
}
