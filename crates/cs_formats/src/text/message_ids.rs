//! The `{name, string id}` table an image keeps beside its string pool
//! (stage F12's id machinery, measured in
//! `docs/findings/2026-10-02-f22-h-original-control-vocabulary.md` and
//! `docs/findings/2026-10-15-m01-hud-objective-message-ids.md`).
//!
//! `strings.dll` carries its `MSG_*`/`FONT_*` vocabulary as a run of 8-byte
//! records — a `u32` name pointer followed by a `u32` `RT_STRING` id — stored
//! **twice and identically** in the image's `.data` section (F22-H measured
//! both runs: 1 023 records each). A name pointer is a virtual address
//! (`ImageBase` + RVA) into the image's own pool of 4-byte-aligned,
//! NUL-terminated ASCII names, which is what makes a record a *pairing* of a
//! symbolic name with the id a [`crate::pe_resources`] `RT_STRING` row answers.
//!
//! Nothing the installation ships names these ids in a header: both generated
//! `#define` headers (`ASSETS/SCRIPTS/RESOURCE.H`, `RESRC1.H`) carry zero
//! `MSG_*` defines, so [`crate::text::resource_header`] cannot pair a message
//! name with an id and this table is the only pairing the installation holds.
//! That is why the reader is production code and not a test probe: the message
//! name an `IDENTITY` declaration carries (`MSG_BRF_NWM1_OBJ1`) becomes a
//! string id through these records alone.
//!
//! # The grammar this reader implements
//!
//! Extracted from that one real sample, and stated so a reader of this file
//! can check it against the findings above:
//!
//! * a record is two little-endian `u32`s at an **8-byte-aligned** offset: a
//!   virtual address and an id;
//! * the id lies in [`MESSAGE_ID_MIN`]..=[`MESSAGE_ID_MAX`] — measured, not
//!   assumed: the installation's ids span 100…17 142, the only smaller word
//!   the scan meets is `0`, and the only larger ones are virtual addresses (an
//!   array of pointers this scan would read as `{pointer, id}`, which the id
//!   bound rejects);
//! * the address is at least `ImageBase` and maps through
//!   [`PeLayout::map_rva`] to raw bytes holding a NUL-terminated name of
//!   [`MESSAGE_NAME_MIN_BYTES`]..=[`MESSAGE_NAME_MAX_BYTES`] bytes in which
//!   every byte is an ASCII letter, digit or `_`.
//!
//! A pair failing any of those is **skipped**, never repaired or guessed, and
//! the scan never fails on content: like [`crate::text::resource_header`], the
//! only errors are a read or a reservation this parse's budgets refuse. A name
//! spelled with two *different* ids is kept twice and reported
//! ([`MessageIdLookup::Ambiguous`]) rather than resolved to one of them; a
//! name repeated with the *same* id — which is what storing the table twice
//! does — is kept once.

use std::fmt;

use crate::error::ParseError;
use crate::io::{AllocationBudget, ParseContext, Reader};
use crate::pe_resources::PeLayout;

/// Entrypoint label [`read_message_ids`] scopes its errors with.
pub const MESSAGE_IDS_ENTRYPOINT: &str = "pe.message_ids";

/// Bytes of one `{name pointer, id}` record.
pub const MESSAGE_ID_RECORD_BYTES: u64 = 8;

/// Shortest name a record may carry: the installation's shortest names are
/// two bytes, and a one-byte name in this position is a fragment, not a label.
pub const MESSAGE_NAME_MIN_BYTES: usize = 2;

/// Longest name a record may carry: the installation's longest name is 34
/// bytes, and the cap stops a run of printable bytes from being booked as a
/// name.
pub const MESSAGE_NAME_MAX_BYTES: usize = 60;

/// Lowest id a record may carry; see the module doc for why the bound is
/// measured rather than assumed.
pub const MESSAGE_ID_MIN: u32 = 1;

/// Highest id a record may carry; see the module doc.
pub const MESSAGE_ID_MAX: u32 = 99_999;

/// One `{name, id}` record of an image's message table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessageIdRecord {
    /// The name the record's pointer addresses: ASCII, NUL-terminated in the
    /// image, and a byte range inside it.
    pub name: String,
    /// The `RT_STRING` id the name labels, as stored.
    pub id: u32,
    /// Where the name's bytes are in the image.
    pub name_offset: u64,
    /// Where the record's eight bytes are in the image.
    pub record_offset: u64,
}

/// The message table of one image: every record the scan accepted, in scan
/// order of [`MessageIdRecord::record_offset`], with a repeat of an identical
/// `(name, id)` pair dropped (the measured table is stored twice) and a
/// *differing* repeat kept so [`Self::resolve`] can report the contradiction.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MessageIdTable {
    records: Vec<MessageIdRecord>,
}

impl MessageIdTable {
    /// The records, in the order the scan met them.
    #[must_use]
    pub fn records(&self) -> &[MessageIdRecord] {
        &self.records
    }

    /// How many records the image carries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether the image carries no message record at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// The id `name` labels, or why there is no single answer.
    ///
    /// Two records spelling `name` with the **same** id cannot both be found:
    /// the read dropped the repeat. Two spelling it with **different** ids are
    /// both here, and the answer is [`MessageIdLookup::Ambiguous`] — choosing
    /// one would hide a contradiction in the image.
    #[must_use]
    pub fn resolve(&self, name: &str) -> MessageIdLookup {
        let mut ids: Vec<u32> = Vec::new();
        for record in &self.records {
            if record.name == name && !ids.contains(&record.id) {
                ids.push(record.id);
            }
        }
        match ids.len() {
            0 => MessageIdLookup::Missing,
            1 => MessageIdLookup::Found(ids[0]),
            _ => MessageIdLookup::Ambiguous(ids),
        }
    }
}

/// The answer to one [`MessageIdTable::resolve`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessageIdLookup {
    /// No record of the image spells this name.
    Missing,
    /// Exactly one record spells it.
    Found(u32),
    /// This many distinct ids answer the name; none is returned, because
    /// picking one would hide the contradiction.
    Ambiguous(Vec<u32>),
}

/// Reads `bytes` as a PE image's message table.
///
/// `layout` is the same image's [`PeLayout`] — the caller has usually read it
/// already through [`crate::pe_resources::read_pe_resources`], and re-reading
/// the headers here would be a second implementation of the section table
/// rather than a second fact about the image.
///
/// The scan visits every 8-byte-aligned offset of `bytes`; every name it
/// accepts is proven to be inside the image by [`PeLayout::map_rva`], so a
/// virtual address that maps nowhere contributes nothing.
///
/// # Errors
///
/// [`MessageIdError`] when a bounded read or an allocation reservation
/// refuses — the only failures this reader has, exactly as
/// [`crate::text::resource_header`] has them. The parse's allocation ledger
/// is rolled back to where this entrypoint found it.
pub fn read_message_ids(
    context: &mut ParseContext,
    layout: &PeLayout,
    bytes: &[u8],
) -> Result<MessageIdTable, MessageIdError> {
    if (bytes.len() as u64) < MESSAGE_ID_RECORD_BYTES {
        return Ok(MessageIdTable::default());
    }
    context
        .parse(
            MESSAGE_IDS_ENTRYPOINT,
            bytes,
            |image: &mut Reader<'_>, allocation: &mut AllocationBudget, _| {
                let mut records: Vec<MessageIdRecord> = Vec::new();
                let last = bytes.len() as u64 - MESSAGE_ID_RECORD_BYTES;
                let mut offset = 0u64;
                while offset <= last {
                    if let Some(record) = record_at(image, allocation, layout, offset)? {
                        records.push(record);
                    }
                    offset += MESSAGE_ID_RECORD_BYTES;
                }
                // The measured table is stored twice, so the same `(name, id)`
                // pair is met twice. The repeats are collapsed after the scan,
                // sorted by name so the pair that survives is the one at the
                // lower record offset; a repeat with a *different* id is a
                // different pair and stays for `resolve` to report.
                records.sort_by(|left, right| {
                    left.name
                        .cmp(&right.name)
                        .then(left.id.cmp(&right.id))
                        .then(left.record_offset.cmp(&right.record_offset))
                });
                records.dedup_by(|first, second| {
                    // `dedup_by` passes the two elements in reverse slice order;
                    // either way one of the two identical records goes and the
                    // other's provenance offsets are kept.
                    first.name == second.name && first.id == second.id
                });
                records.sort_by_key(|record| record.record_offset);
                Ok(MessageIdTable { records })
            },
        )
        .map_err(MessageIdError::from)
}

/// The record at `offset`, or `None` when the eight bytes there are not a
/// `{name pointer, id}` pair this grammar accepts.
///
/// `Ok(None)` is the ordinary case — most offsets in an image hold no record —
/// so nothing here is an error except a read or a reservation that refuses.
fn record_at(
    image: &mut Reader<'_>,
    allocation: &mut AllocationBudget,
    layout: &PeLayout,
    offset: u64,
) -> Result<Option<MessageIdRecord>, ParseError> {
    let mut row = image.window(offset, MESSAGE_ID_RECORD_BYTES, "pe.message_ids.record")?;
    let pointer = row.read_u32("pe.message_ids.pointer")?;
    let id = row.read_u32("pe.message_ids.id")?;
    if !(MESSAGE_ID_MIN..=MESSAGE_ID_MAX).contains(&id) {
        return Ok(None);
    }
    let Some(rva) = u64::from(pointer)
        .checked_sub(layout.image_base())
        .and_then(|rva| u32::try_from(rva).ok())
    else {
        return Ok(None);
    };
    let Some(mapped) = layout.map_rva(rva) else {
        return Ok(None);
    };
    if mapped.available == 0 {
        return Ok(None);
    }
    // One byte past the cap is read on purpose: a name that continues past
    // the cap then has no NUL inside the window and fails the length check,
    // instead of being truncated into a name the image never spelled.
    let want = mapped
        .available
        .min(u64::from(MESSAGE_NAME_MAX_BYTES as u32) + 1);
    let window = image.window_bytes(mapped.file_offset, want, "pe.message_ids.name")?;
    let Some(name_end) = window.iter().position(|byte| !is_name_byte(*byte)) else {
        return Ok(None);
    };
    if !(MESSAGE_NAME_MIN_BYTES..=MESSAGE_NAME_MAX_BYTES).contains(&name_end) {
        return Ok(None);
    }
    let raw = &window[..name_end];
    allocation.reserve(
        "pe.message_ids.record",
        offset,
        1,
        std::mem::size_of::<MessageIdRecord>() as u64 + raw.len() as u64,
    )?;
    Ok(Some(MessageIdRecord {
        name: String::from_utf8_lossy(raw).into_owned(),
        id,
        name_offset: mapped.file_offset,
        record_offset: offset,
    }))
}

/// Whether a byte may appear inside a message name: an ASCII letter, digit or
/// `_`, which is every byte the installation's names use.
const fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Why a message-table read could not be reported as a table, named so a
/// consumer can carry it beside other text errors without erasing which
/// entrypoint refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessageIdError(ParseError);

impl MessageIdError {
    /// Stable, machine-matchable label.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        "message_ids"
    }
}

impl From<ParseError> for MessageIdError {
    fn from(error: ParseError) -> Self {
        Self(error)
    }
}

impl fmt::Display for MessageIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

impl std::error::Error for MessageIdError {}
