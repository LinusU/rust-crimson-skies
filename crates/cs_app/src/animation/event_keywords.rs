//! The engine's own keyword for every stored anim event id (F13-D-FU2, #1416).
//!
//! F20 (#690) named 31 of the 35 stored compiled event ids by aligning the
//! installation's declaration statements with the compiled events
//! ([`super::events::STORED_OPCODES`]); ids **13, 17, 26 and 28** joined no
//! statement there. This table closes the question for **every** stored id
//! with a second, independent source: the owner-supplied decrypted engine
//! image (`crimson.decrypted.exe`, `$CS_ENGINE_IMAGE`, never committed), where
//! the reader-text parser of each keyword stores its event id and record
//! length, and where the animation runtime registers one handler per id.
//!
//! What is committed is a **small factual table** (AGENTS.md rule 3, owner
//! exception of 2026-10-14): virtual addresses, ids, lengths and keyword
//! spellings. No bytes, disassembly or decompiled code. Every row is
//! provenance `static`; nothing here is `verified_original`, because no
//! original run was observed. The finding
//! `docs/findings/2026-10-18-f13-d-fu2-unjoined-anim-event-ids.md` holds the
//! method and the per-id evidence.
//!
//! Naming an id here does **not** decode it: [`super::events::decode_event_stream`]
//! still refuses 13, 17, 26 and 28 (see [`UNJOINED_REFUSAL`]).

/// The claim id every row of [`ENGINE_EVENT_KEYWORDS`] and
/// [`UNJOINED_EVENTS`] cites.
pub const ENGINE_KEYWORD_CLAIM: &str = "f13-d-fu2.event-keyword-engine-static";

/// Why the 496 events of ids 13, 17, 26 and 28 stay refused although the ids
/// are now named.
pub const UNJOINED_REFUSAL: &str = "the id is named by the engine's parser, but its payload fields are only partly measured and no original run shows its effect, so its timing is not read";

/// One stored event id and the engine's keyword for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineEventKeyword {
    /// The compiled event id (the tag word's low byte).
    pub opcode: u8,
    /// The reader-text keyword whose parser stores this id.
    pub keyword: &'static str,
    /// Virtual address, in the engine image, of the instruction that stores
    /// the id into a new event record.
    pub writer_va: u32,
    /// The fixed record length (bytes, header included) the parser stores next
    /// to the id (id 47 appends the script it reads, so its stored events are
    /// longer). `None` where the parser computes it instead of storing a
    /// constant (`SOUND_ADJUST`).
    pub record_bytes: Option<u32>,
    /// Virtual address of the animation runtime's handler registered for this
    /// id in the dispatch table at VA `0x727de0` (indexed by id, ids 1..=47).
    pub handler_va: u32,
}

const fn row(
    opcode: u8,
    keyword: &'static str,
    writer_va: u32,
    record_bytes: Option<u32>,
    handler_va: u32,
) -> EngineEventKeyword {
    EngineEventKeyword {
        opcode,
        keyword,
        writer_va,
        record_bytes,
        handler_va,
    }
}

/// Where an id's keyword was also found in the installation's reader text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TwinEvidence {
    /// F20's unique declaration/record alignment names the id (31 ids).
    F20Join,
    /// Not in F20's unique join, but the keyword's statements align to this
    /// id at every non-unique candidate record too, with no other spelling
    /// at that id: `statements` aligned events.
    CandidateAlignment {
        /// Aligned events counted over every candidate record.
        statements: u32,
    },
    /// No declaration statement of the installation carries the keyword at an
    /// alignable position; the name rests on the engine image alone.
    EngineOnly,
}

/// One of the four ids F20 left unjoined, with its measured facts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnjoinedEvent {
    /// The compiled event id.
    pub opcode: u8,
    /// The engine's keyword (also the table row's keyword).
    pub keyword: &'static str,
    /// The keyword in the reader text of the installation, when aligned.
    pub twin: TwinEvidence,
    /// The reader keywords the parser accepts for this statement, in the
    /// order the parser tests them (statement-level, `START_TIME` first).
    pub parameters: &'static [&'static str],
    /// Timing: the parser reads `START_TIME` and stores it in the record's
    /// first payload word (offset 8); no `RUN_TIME` keyword exists.
    pub timing: &'static str,
    /// Role, as far as the parser and handler show it.
    pub role: &'static str,
    /// Events of this id over all 56 994 blocks (F20 census).
    pub events: u32,
}

/// The four ids F20 left unjoined. Not `verified_original`.
pub const UNJOINED_EVENTS: [UnjoinedEvent; 4] = [
    UnjoinedEvent {
        opcode: 13,
        keyword: "OBJECT_OPACITY_STATE",
        twin: TwinEvidence::CandidateAlignment { statements: 10 },
        parameters: &["START_TIME", "NAME", "MAIN_ROOT_NODE", "STATE"],
        timing: "START_TIME at payload word 0; no RUN_TIME keyword",
        role: "sets the named node's opacity state ON (1) or OFF (0); STATE absent stays 0xffff",
        events: 340,
    },
    UnjoinedEvent {
        opcode: 17,
        keyword: "OBJECT_CYCLE_TEXTURE",
        twin: TwinEvidence::CandidateAlignment { statements: 96 },
        parameters: &["NAME", "START_TIME", "RESET"],
        timing: "START_TIME at payload word 0; no RUN_TIME keyword",
        role: "cycles the named node's texture; RESET sets bit 0 of the flag word at record offset 12",
        events: 144,
    },
    UnjoinedEvent {
        opcode: 26,
        keyword: "RESET_ANIMATION",
        twin: TwinEvidence::CandidateAlignment { statements: 10 },
        parameters: &["NAME", "START_TIME"],
        timing: "START_TIME at payload word 0; no RUN_TIME keyword",
        role: "names an animation (32-byte name field at record offset 12) and the runtime resolves it to an instance index",
        events: 11,
    },
    UnjoinedEvent {
        opcode: 28,
        keyword: "FOG_STATE",
        twin: TwinEvidence::EngineOnly,
        parameters: &["NAME", "START_TIME", "TYPE", "COLOR", "ALTITUDE", "RANGE"],
        timing: "START_TIME at payload word 0; no RUN_TIME keyword",
        role: "sets the scene fog: TYPE OFF (0), LINEAR (1) or EXPONENTIAL (2) at record offset 48, with presence bits for COLOR, ALTITUDE and RANGE in the flag word at offset 44",
        events: 1,
    },
];

/// Every stored id with the engine's keyword for it (35 rows, ascending).
pub const ENGINE_EVENT_KEYWORDS: [EngineEventKeyword; 35] = [
    row(1, "SOUND", 0x4ef630, Some(28), 0x4e74e0),
    row(2, "SOUND_NODE", 0x4f378e, Some(72), 0x4e7a10),
    row(4, "LIGHT_STATE", 0x4f4075, Some(152), 0x4e7b60),
    row(5, "LIGHT_ANIMATION", 0x4fdaea, Some(112), 0x4e82b0),
    row(6, "OBJECT_ACTIVE_STATE", 0x5083db, Some(20), 0x4e8f40),
    row(7, "OBJECT_TRANSLATE_STATE", 0x507609, Some(32), 0x4e8de0),
    row(8, "OBJECT_SCALE_STATE", 0x506ebd, Some(28), 0x4e8da0),
    row(9, "OBJECT_ROTATE_STATE", 0x506049, Some(32), 0x4e8b80),
    row(10, "OBJECT_MOTION", 0x5085eb, Some(332), 0x4e8fa0),
    row(11, "OBJECT_MOTION_FROM_TO", 0x50cd5c, Some(144), 0x4e9ee0),
    row(12, "OBJECT_MOTION_SI_SCRIPT", 0x50fd64, Some(76), 0x4ea7d0),
    row(13, "OBJECT_OPACITY_STATE", 0x511404, Some(24), 0x4ea960),
    row(14, "OBJECT_OPACITY_FROM_TO", 0x51189e, Some(36), 0x4ea9e0),
    row(15, "OBJECT_ADD_CHILD", 0x512220, Some(16), 0x4eab20),
    row(16, "OBJECT_DELETE_CHILD", 0x512500, Some(16), 0x4eab90),
    row(17, "OBJECT_CYCLE_TEXTURE", 0x5127e2, Some(20), 0x4eabd0),
    row(20, "CAMERA_STATE", 0x502dfd, Some(48), 0x4e85c0),
    row(22, "CALL_SEQUENCE", 0x5159d9, Some(48), 0x4eb570),
    row(23, "STOP_SEQUENCE", 0x515b19, Some(48), 0x4eb610),
    row(24, "CALL_ANIMATION", 0x515c64, Some(80), 0x4eb6b0),
    row(25, "STOP_ANIMATION", 0x5160a4, Some(48), 0x4ebc80),
    row(26, "RESET_ANIMATION", 0x5161d9, Some(48), 0x4ebe40),
    row(27, "INVALIDATE_ANIMATION", 0x516309, Some(48), 0x4ebee0),
    row(28, "FOG_STATE", 0x4ff31a, Some(80), 0x4e8540),
    row(30, "LOOP", 0x516474, Some(20), 0x4ebfd0),
    row(31, "IF", 0x5167f0, Some(28), 0x4ec080),
    row(32, "ELSE", 0x518395, Some(12), 0x4ec5a0),
    row(33, "ELSEIF", 0x518320, Some(28), 0x4ec5a0),
    row(34, "ENDIF", 0x518425, Some(12), 0x4ec5d0),
    row(35, "CALLBACK", 0x5184c0, Some(16), 0x4ec5e0),
    row(36, "FBFX_COLOR_FROM_TO", 0x518d09, Some(64), 0x4ec6a0),
    row(41, "DETONATE_WEAPON", 0x518575, Some(36), 0x4ec610),
    row(42, "PUFFER_STATE", 0x4f717b, Some(596), 0x4e7e40),
    row(46, "SOUND_ADJUST", 0x4f0299, None, 0x4e75b0),
    row(47, "OBJECT_MOTION_SI_SCRIPT", 0x510f15, Some(16), 0x4ea900),
];

/// The engine row for `opcode`, when the id is a stored one.
#[must_use]
pub fn engine_keyword(opcode: u8) -> Option<EngineEventKeyword> {
    ENGINE_EVENT_KEYWORDS
        .iter()
        .copied()
        .find(|row| row.opcode == opcode)
}

/// The measured facts of an F20-unjoined id.
#[must_use]
pub fn unjoined_event(opcode: u8) -> Option<UnjoinedEvent> {
    UNJOINED_EVENTS
        .iter()
        .copied()
        .find(|event| event.opcode == opcode)
}
