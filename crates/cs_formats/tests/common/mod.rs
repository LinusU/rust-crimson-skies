//! Synthetic fixture for the F03-A acceptance tests.
//!
//! Newly authored little-endian bytes only: no original game data, no
//! `CS_GAME_DIR` access. The record is deliberately small so a test can
//! truncate it at every byte boundary.
//
// Each integration-test binary links this shared module and uses only part of
// it, so `dead_code` would fire on the helpers another binary uses; the
// assertions themselves stay fully checked.
#![allow(dead_code)]

use cs_formats::{ParseError, Reader};

/// Provenance label carried by every error these tests assert on.
pub const CONTAINER: &str = "synthetic/f03_a_record.bin";

pub const EXPECTED_MAGIC: u32 = 0x524F_4353; // "SCOR" little-endian
pub const EXPECTED_VERSION: u16 = 3;
pub const EXPECTED_LABEL: &str = "crimson";
pub const EXPECTED_SCALE: f32 = 2.5;
pub const EXPECTED_COUNT: u32 = 7;
pub const EXPECTED_FLAGS: u8 = 0b0000_0101;

/// Length of the `header.label` fixed string field.
pub const LABEL_LEN: usize = 8;

/// Offset of `header.count` inside the record: the observable-failure anchor
/// named in the task (truncating here must report `header.count` at 18).
pub const COUNT_OFFSET: u64 = 18;

/// Total record length.
pub fn record_len() -> usize {
    4 + 2 + LABEL_LEN + 4 + 4 + 1
}

/// The synthetic record: magic, version, label, scale, count, flags.
pub fn record_bytes() -> Vec<u8> {
    let mut bytes = Vec::with_capacity(record_len());
    bytes.extend_from_slice(&EXPECTED_MAGIC.to_le_bytes());
    bytes.extend_from_slice(&EXPECTED_VERSION.to_le_bytes());
    let mut label = [0u8; LABEL_LEN];
    label[..EXPECTED_LABEL.len()].copy_from_slice(EXPECTED_LABEL.as_bytes());
    bytes.extend_from_slice(&label);
    bytes.extend_from_slice(&EXPECTED_SCALE.to_le_bytes());
    bytes.extend_from_slice(&EXPECTED_COUNT.to_le_bytes());
    bytes.push(EXPECTED_FLAGS);
    debug_assert_eq!(bytes.len(), record_len());
    bytes
}

/// The parsed shape of [`record_bytes`]: F03-A's typed output.
#[derive(Debug, PartialEq)]
pub struct Record {
    pub magic: u32,
    pub version: u16,
    pub label: String,
    pub scale: f32,
    pub count: u32,
    pub flags: u8,
}

/// Reads one record with named fields, the way a real parser entrypoint does.
pub fn parse_record(reader: &mut Reader<'_>) -> Result<Record, ParseError> {
    let magic = reader.read_u32("header.magic")?;
    let version = reader.read_u16("header.version")?;
    let label = reader
        .read_bounded_cstr("header.label", LABEL_LEN)?
        .to_owned();
    let scale = reader.read_f32("header.scale")?;
    let count = reader.read_u32("header.count")?;
    let flags = reader.read_u8("header.flags")?;
    Ok(Record {
        magic,
        version,
        label,
        scale,
        count,
        flags,
    })
}
