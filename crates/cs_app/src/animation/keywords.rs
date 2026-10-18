//! The owner's anim-event keyword list against F20's stored-opcode table
//! (task #1415, `F13-D-FU1`).
//!
//! Two vocabularies describe the animation event family. The owner's static
//! analysis of the decrypted executable (note on #640, 2026-10-05) lists the
//! keywords of the text parser's table at `0x63341c`–`0x633718`
//! ([`OWNER_KEYWORDS`]); F20's grammar (#690, [`super::events::STORED_OPCODES`])
//! carries the spellings the installation's own declarations state for the
//! opcodes the compiled carriers store. This module places **every** name of
//! both exactly once, by a rule that reads only evidence:
//!
//! * a keyword spelled exactly like an F20 statement is a **compiled-event
//!   name** — F20 joined that spelling to a stored opcode statement for
//!   statement;
//! * otherwise, a keyword found as a definition-level or record-level field of
//!   the installation's reader text is a **reader-text record-vocabulary
//!   field**;
//! * otherwise, a keyword found as a statement kind of the reader text, but
//!   with no stored opcode, is a **reader-text statement only**;
//! * otherwise the keyword is **neither**: the installation's reader text
//!   never writes it and no stored opcode carries it.
//!
//! A stored opcode that joins no spelling stays unjoined. Nothing here assigns
//! an owner keyword to opcode 13, 17, 26 or 28: that is #1416's measurement.
//! The classification is a measurement over file contents, never
//! `verified_original`, and it claims no model for any anim family.

use std::collections::{BTreeMap, BTreeSet};

use super::events::{STORED_OPCODES, opcode_info};
use super::programs::AnimationDefinitionMember;

/// The keywords of the owner's note, in the note's order, each exactly once.
///
/// The note says "41 event keywords" and enumerates 44 names; all 44 are kept.
pub const OWNER_KEYWORDS: [&str; 44] = [
    "SOUND",
    "SOUND_NODE",
    "SOUND_ADJUST",
    "EFFECT",
    "LIGHT_STATE",
    "LIGHT_ANIMATION",
    "FOG_STATE",
    "OBJECT_ACTIVE_STATE",
    "OBJECT_TRANSLATE_STATE",
    "OBJECT_SCALE_STATE",
    "OBJECT_ROTATE_STATE",
    "OBJECT_MOTION",
    "OBJECT_MOTION_FROM_TO",
    "OBJECT_MOTION_SI_SCRIPT",
    "OBJECT_OPACITY_STATE",
    "OBJECT_OPACITY_FROM_TO",
    "OBJECT_ADD_CHILD",
    "OBJECT_DELETE_CHILD",
    "OBJECT_CYCLE_TEXTURE",
    "OBJECT_CONNECTOR",
    "OBJECT_VERTEX_COLOR",
    "CALL_OBJECT_CONNECTOR",
    "CAMERA_STATE",
    "CAMERA_FROM_TO",
    "CALL_SEQUENCE",
    "STOP_SEQUENCE",
    "CALL_ANIMATION",
    "STOP_ANIMATION",
    "RESET_ANIMATION",
    "INVALIDATE_ANIMATION",
    "LOOP",
    "IF",
    "ELSEIF",
    "ELSE",
    "ENDIF",
    "CALLBACK",
    "DETONATE_WEAPON",
    "PUFFER_STATE",
    "FBFX_COLOR_FROM_TO",
    "FBFX_CSINWAVE_FROM_TO",
    "ANIM_VERBOSE",
    "GLOBAL_ANIM_VERBOSE",
    "ON_CALL",
    "ACTIVATION",
];

/// What the installation's reader text states, counted by name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReaderVocabulary {
    /// Statement kinds of `SEQUENCE_DEFINITION` entries, with occurrences.
    pub statement_kinds: BTreeMap<String, usize>,
    /// Field names a definition (or an `ANIMATION_DEFINITIONS` record body)
    /// stores, with occurrences. Includes the fields the reader interprets
    /// (`ACTIVATION`, `NAME`, …) and every uninterpreted one.
    pub record_fields: BTreeMap<String, usize>,
}

impl ReaderVocabulary {
    /// Adds one read `ANIMATION_DEFINITIONS` member.
    pub fn add_member(&mut self, member: &AnimationDefinitionMember) {
        for field in member.fields() {
            *self.record_fields.entry(field.clone()).or_default() += 1;
        }
        for definition in member.definitions() {
            if definition.activation().is_some() {
                *self
                    .record_fields
                    .entry("ACTIVATION".to_owned())
                    .or_default() += 1;
            }
            for field in definition.uninterpreted_fields() {
                *self.record_fields.entry(field.clone()).or_default() += 1;
            }
            for sequence in definition.sequences() {
                for entry in sequence.entries() {
                    *self
                        .statement_kinds
                        .entry(entry.kind().to_owned())
                        .or_default() += 1;
                }
            }
        }
    }
}

/// Where one name sits, with the evidence for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Placement {
    /// F20 joined this spelling to these stored opcodes.
    CompiledEventName {
        /// Every stored opcode whose statement is this spelling, ascending.
        opcodes: Vec<u8>,
    },
    /// The reader text stores it as a record or definition field.
    ReaderRecordField {
        /// Occurrences as a field in the reader text.
        occurrences: usize,
        /// Occurrences of the same spelling as a `SEQUENCE_DEFINITION`
        /// statement kind (`ACTIVATION` is both, and F20 drops it from the
        /// stored events).
        also_statement: usize,
    },
    /// The reader text writes it as a statement, but no opcode is stored.
    ReaderStatementOnly {
        /// Occurrences in the reader text.
        occurrences: usize,
    },
    /// Neither the reader text nor the stored opcodes mention it.
    Neither,
}

impl Placement {
    /// The stable label used in findings and reports.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::CompiledEventName { .. } => "compiled_event_name",
            Self::ReaderRecordField { .. } => "reader_record_vocabulary_field",
            Self::ReaderStatementOnly { .. } => "reader_statement_without_stored_opcode",
            Self::Neither => "neither",
        }
    }
}

/// One owner keyword and its placement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeywordRow {
    /// The owner's spelling.
    pub keyword: &'static str,
    /// Where it sits.
    pub placement: Placement,
}

/// The whole reconciliation of the two vocabularies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reconciliation {
    /// One row per owner keyword, in the owner's order.
    pub owner: Vec<KeywordRow>,
    /// Stored opcodes that join an owner keyword: `(opcode, spelling)`.
    pub joined_opcodes: Vec<(u8, &'static str)>,
    /// Stored opcodes whose spelling is not an owner keyword (extra on F20's
    /// side). Empty when the vocabularies agree.
    pub extra_f20: Vec<(u8, &'static str)>,
    /// Stored opcodes that join no spelling at all.
    pub unjoined_opcodes: Vec<u8>,
    /// Statement kinds the reader text writes that are not owner keywords,
    /// with occurrences (extra on the reader side).
    pub reader_statement_extras: Vec<(String, usize)>,
    /// Field names the reader text stores that are not owner keywords, with
    /// occurrences (a different vocabulary: definition configuration).
    pub reader_field_extras: Vec<(String, usize)>,
}

impl Reconciliation {
    /// Owner keywords with the given placement label.
    pub fn with_label<'a>(&'a self, label: &'a str) -> impl Iterator<Item = &'a KeywordRow> {
        self.owner
            .iter()
            .filter(move |row| row.placement.label() == label)
    }
}

/// Reconciles [`OWNER_KEYWORDS`] with [`STORED_OPCODES`] over a reader
/// vocabulary.
#[must_use]
pub fn reconcile(reader: &ReaderVocabulary) -> Reconciliation {
    let owner_set: BTreeSet<&str> = OWNER_KEYWORDS.iter().copied().collect();
    let mut owner = Vec::new();
    for keyword in OWNER_KEYWORDS {
        let opcodes: Vec<u8> = STORED_OPCODES
            .iter()
            .filter(|info| info.statement == keyword)
            .map(|info| info.opcode)
            .collect();
        let placement = if !opcodes.is_empty() {
            Placement::CompiledEventName { opcodes }
        } else if let Some(&occurrences) = reader.record_fields.get(keyword) {
            Placement::ReaderRecordField {
                occurrences,
                also_statement: reader.statement_kinds.get(keyword).copied().unwrap_or(0),
            }
        } else if let Some(&occurrences) = reader.statement_kinds.get(keyword) {
            Placement::ReaderStatementOnly { occurrences }
        } else {
            Placement::Neither
        };
        owner.push(KeywordRow { keyword, placement });
    }
    let mut joined_opcodes = Vec::new();
    let mut extra_f20 = Vec::new();
    let mut unjoined_opcodes = Vec::new();
    for info in STORED_OPCODES {
        if info.statement.is_empty() {
            unjoined_opcodes.push(info.opcode);
        } else if owner_set.contains(info.statement) {
            joined_opcodes.push((info.opcode, info.statement));
        } else {
            extra_f20.push((info.opcode, info.statement));
        }
    }
    debug_assert!(unjoined_opcodes.iter().all(|op| opcode_info(*op).is_some()));
    let extras = |map: &BTreeMap<String, usize>| -> Vec<(String, usize)> {
        map.iter()
            .filter(|(name, _)| !owner_set.contains(name.as_str()))
            .map(|(name, count)| (name.clone(), *count))
            .collect()
    };
    Reconciliation {
        reader_statement_extras: extras(&reader.statement_kinds),
        reader_field_extras: extras(&reader.record_fields),
        owner,
        joined_opcodes,
        extra_f20,
        unjoined_opcodes,
    }
}
