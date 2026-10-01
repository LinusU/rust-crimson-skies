//! Checksummed line encoding of a [`ProfileDocument`].
//!
//! ```text
//! CSSAVE <major>.<minor>
//! profile_id=<n>
//! kind=<label>
//! display_name=<text>
//! revision=<n>
//! campaign.run=<key>              (optional)
//! campaign.money_minor=<n>
//! campaign.outcome=<key>          (repeated)
//! blueprint=<content id>          (repeated)
//! record.<key>=<n>
//! setting.<key>=<live|restart>:<text>
//! fingerprint.<name>=<16 hex>
//! <unknown key>=<text>            (preserved verbatim)
//! checksum=<16 hex FNV-1a 64 of every byte above this line>
//! ```
//!
//! The checksum is an integrity check against torn or corrupted files, not a
//! security measure. Everything is bounded before it is interpreted.
//!
//! [`sealed_body`] and [`check_line_count`] are the framing this module shares
//! with the profile registry (`super::library`), which is a different document
//! with the same durability rules; a body is only ever interpreted by the
//! decoder that owns its format.

use std::fmt;

use cs_types::content::ContentId;
use cs_types::profile::{
    CampaignState, ExtraField, FingerprintEntry, ProfileDocument, ProfileFieldError, ProfileId,
    ProfileKind, RecordEntry, Revision, SchemaVersion, SettingApply, SettingEntry, validate_key,
};

/// Largest accepted encoded document.
pub const MAX_SAVE_BYTES: usize = 256 * 1024;
/// Largest accepted number of lines.
pub const MAX_SAVE_LINES: usize = 4096;

const MAGIC: &str = "CSSAVE";

/// The key of the trailing integrity line every sealed document ends with.
pub const CHECKSUM_KEY: &str = "checksum=";

/// Why a byte string is not an acceptable save.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeError {
    TooLarge {
        len: usize,
    },
    NotUtf8,
    BadHeader,
    /// The major version is not this build's. The bytes are untouched; the
    /// caller must not overwrite them.
    UnsupportedMajor {
        found: SchemaVersion,
    },
    TooManyLines,
    MissingChecksum,
    ChecksumMismatch,
    Malformed {
        line: usize,
        reason: &'static str,
    },
    MissingField(&'static str),
    Field(ProfileFieldError),
    /// The document's fields decoded but do not hold together: a profile
    /// registry whose mark is below a live id, or which lists one id twice.
    /// The bytes are intact, so this is not a torn write.
    Inconsistent {
        document: &'static str,
        reason: String,
    },
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { len } => write!(f, "save is {len} bytes, over {MAX_SAVE_BYTES}"),
            Self::NotUtf8 => write!(f, "save is not UTF-8"),
            Self::BadHeader => write!(f, "save header is not `{MAGIC} <major>.<minor>`"),
            Self::UnsupportedMajor { found } => {
                write!(f, "save schema {found} is not readable by this build")
            }
            Self::TooManyLines => write!(f, "save has more than {MAX_SAVE_LINES} lines"),
            Self::MissingChecksum => write!(f, "save has no trailing checksum line"),
            Self::ChecksumMismatch => write!(f, "save checksum does not match its content"),
            Self::Malformed { line, reason } => write!(f, "line {line}: {reason}"),
            Self::MissingField(name) => write!(f, "save lacks required field {name}"),
            Self::Field(error) => write!(f, "{error}"),
            Self::Inconsistent { document, reason } => {
                write!(f, "{document} does not hold together: {reason}")
            }
        }
    }
}

impl std::error::Error for DecodeError {}

impl From<ProfileFieldError> for DecodeError {
    fn from(error: ProfileFieldError) -> Self {
        Self::Field(error)
    }
}

/// FNV-1a 64-bit over `bytes`.
pub fn checksum(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Encodes a validated document.
pub fn encode(document: &ProfileDocument) -> Result<Vec<u8>, ProfileFieldError> {
    document.validate()?;
    let mut out = String::new();
    let schema = document.schema;
    out.push_str(&format!("{MAGIC} {schema}\n"));
    out.push_str(&format!("profile_id={}\n", document.profile_id));
    out.push_str(&format!("kind={}\n", document.kind.label()));
    out.push_str(&format!("display_name={}\n", document.display_name));
    out.push_str(&format!("revision={}\n", document.revision.0));
    if let Some(run) = &document.campaign.run_id {
        out.push_str(&format!("campaign.run={run}\n"));
    }
    out.push_str(&format!(
        "campaign.money_minor={}\n",
        document.campaign.money_minor
    ));
    for outcome in &document.campaign.applied_outcomes {
        out.push_str(&format!("campaign.outcome={outcome}\n"));
    }
    for blueprint in &document.blueprints {
        out.push_str(&format!("blueprint={blueprint}\n"));
    }
    for record in &document.records {
        out.push_str(&format!("record.{}={}\n", record.key, record.value));
    }
    for setting in &document.settings {
        out.push_str(&format!(
            "setting.{}={}:{}\n",
            setting.key,
            setting.apply.label(),
            setting.value
        ));
    }
    for fingerprint in &document.fingerprints {
        out.push_str(&format!(
            "fingerprint.{}={:016x}\n",
            fingerprint.name, fingerprint.hash
        ));
    }
    for extra in &document.extra {
        out.push_str(&format!("{}={}\n", extra.key, extra.value));
    }
    let sum = checksum(out.as_bytes());
    out.push_str(&format!("{CHECKSUM_KEY}{sum:016x}\n"));
    Ok(out.into_bytes())
}

/// A bounded decimal field. Every number in a save is read through this, so a
/// value that is not digits or does not fit is refused before it is used.
pub fn parse_u64(line: usize, text: &str) -> Result<u64, DecodeError> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(DecodeError::Malformed {
            line,
            reason: "expected a decimal number",
        });
    }
    text.parse().map_err(|_| DecodeError::Malformed {
        line,
        reason: "number out of range",
    })
}

/// A `ProfileId` field: a nonzero persistent number.
pub fn parse_profile_id(line: usize, text: &str) -> Result<ProfileId, DecodeError> {
    ProfileId::new(parse_u64(line, text)?).ok_or(DecodeError::Malformed {
        line,
        reason: "profile id must not be zero",
    })
}

fn parse_version(magic: &str, header: &str) -> Result<SchemaVersion, DecodeError> {
    let rest = header
        .strip_prefix(magic)
        .and_then(|r| r.strip_prefix(' '))
        .ok_or(DecodeError::BadHeader)?;
    let (major, minor) = rest.split_once('.').ok_or(DecodeError::BadHeader)?;
    let number = |text: &str| -> Result<u16, DecodeError> {
        if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
            return Err(DecodeError::BadHeader);
        }
        text.parse().map_err(|_| DecodeError::BadHeader)
    };
    Ok(SchemaVersion {
        major: number(major)?,
        minor: number(minor)?,
    })
}

/// Opens a sealed line document: returns the body its checksum covers and the
/// schema its header declares.
///
/// This is the shared framing of every persisted engine document — a profile
/// save and the profile registry — so an oversized, non-UTF-8, unsealed,
/// checksum-broken or unreadable-schema document is refused the same way
/// whatever it holds. The body is *covered* text: it excludes the header line
/// and the trailing `checksum=` line, and it has already been verified against
/// the stored checksum. What the body means is the caller's business.
pub fn sealed_body<'a>(
    magic: &str,
    bytes: &'a [u8],
) -> Result<(&'a str, SchemaVersion), DecodeError> {
    if bytes.len() > MAX_SAVE_BYTES {
        return Err(DecodeError::TooLarge { len: bytes.len() });
    }
    let text = std::str::from_utf8(bytes).map_err(|_| DecodeError::NotUtf8)?;
    let header_end = text.find('\n').ok_or(DecodeError::BadHeader)?;
    let schema = parse_version(magic, &text[..header_end])?;
    if !schema.is_readable() {
        return Err(DecodeError::UnsupportedMajor { found: schema });
    }
    let body = text
        .strip_suffix('\n')
        .ok_or(DecodeError::MissingChecksum)?;
    let checksum_start = body
        .rfind('\n')
        .map(|at| at + 1)
        .ok_or(DecodeError::MissingChecksum)?;
    let stored = body[checksum_start..]
        .strip_prefix(CHECKSUM_KEY)
        .ok_or(DecodeError::MissingChecksum)?;
    if stored.len() != 16
        || !stored
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(DecodeError::MissingChecksum);
    }
    let stored = u64::from_str_radix(stored, 16).map_err(|_| DecodeError::MissingChecksum)?;
    if checksum(&bytes[..checksum_start]) != stored {
        return Err(DecodeError::ChecksumMismatch);
    }
    Ok((&text[header_end + 1..checksum_start], schema))
}

/// Refuses a sealed document with more lines than a bounded reader accepts.
pub fn check_line_count(body: &str) -> Result<(), DecodeError> {
    if body.lines().count() > MAX_SAVE_LINES {
        return Err(DecodeError::TooManyLines);
    }
    Ok(())
}

/// Decodes and fully validates a save. Never panics on any input.
pub fn decode(bytes: &[u8]) -> Result<ProfileDocument, DecodeError> {
    let (covered, schema) = sealed_body(MAGIC, bytes)?;
    check_line_count(covered)?;

    let mut profile_id = None;
    let mut kind = None;
    let mut display_name = None;
    let mut revision = None;
    let mut campaign = CampaignState::default();
    let mut money_seen = false;
    let mut blueprints = Vec::new();
    let mut records = Vec::new();
    let mut settings = Vec::new();
    let mut fingerprints = Vec::new();
    let mut extra = Vec::new();

    for (index, line) in covered.lines().enumerate() {
        let number = index + 2;
        let (key, value) = line.split_once('=').ok_or(DecodeError::Malformed {
            line: number,
            reason: "expected key=value",
        })?;
        let once = |slot_set: bool| {
            if slot_set {
                Err(DecodeError::Malformed {
                    line: number,
                    reason: "field repeated",
                })
            } else {
                Ok(())
            }
        };
        match key {
            "profile_id" => {
                once(profile_id.is_some())?;
                profile_id = Some(parse_profile_id(number, value)?);
            }
            "kind" => {
                once(kind.is_some())?;
                kind = Some(
                    ProfileKind::from_label(value).ok_or(DecodeError::Malformed {
                        line: number,
                        reason: "unknown profile kind",
                    })?,
                );
            }
            "display_name" => {
                once(display_name.is_some())?;
                display_name = Some(value.to_owned());
            }
            "revision" => {
                once(revision.is_some())?;
                revision = Some(Revision(parse_u64(number, value)?));
            }
            "campaign.run" => {
                once(campaign.run_id.is_some())?;
                campaign.run_id = Some(value.to_owned());
            }
            "campaign.money_minor" => {
                once(money_seen)?;
                money_seen = true;
                campaign.money_minor = parse_u64(number, value)?;
            }
            "campaign.outcome" => campaign.applied_outcomes.push(value.to_owned()),
            "blueprint" => {
                blueprints.push(ContentId::parse(value).map_err(|_| DecodeError::Malformed {
                    line: number,
                    reason: "invalid blueprint content id",
                })?)
            }
            _ => {
                if let Some(name) = key.strip_prefix("record.") {
                    records.push(RecordEntry {
                        key: name.to_owned(),
                        value: parse_u64(number, value)?,
                    });
                } else if let Some(name) = key.strip_prefix("setting.") {
                    let (apply, text) = value.split_once(':').ok_or(DecodeError::Malformed {
                        line: number,
                        reason: "setting needs `live:` or `restart:`",
                    })?;
                    settings.push(SettingEntry {
                        key: name.to_owned(),
                        apply: SettingApply::from_label(apply).ok_or(DecodeError::Malformed {
                            line: number,
                            reason: "unknown setting apply mode",
                        })?,
                        value: text.to_owned(),
                    });
                } else if let Some(name) = key.strip_prefix("fingerprint.") {
                    if value.len() != 16
                        || !value
                            .bytes()
                            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
                    {
                        return Err(DecodeError::Malformed {
                            line: number,
                            reason: "fingerprint must be 16 lowercase hex digits",
                        });
                    }
                    fingerprints.push(FingerprintEntry {
                        name: name.to_owned(),
                        hash: u64::from_str_radix(value, 16).map_err(|_| {
                            DecodeError::Malformed {
                                line: number,
                                reason: "fingerprint is not hex",
                            }
                        })?,
                    });
                } else {
                    validate_key("extra key", key)?;
                    extra.push(ExtraField {
                        key: key.to_owned(),
                        value: value.to_owned(),
                    });
                }
            }
        }
    }

    let document = ProfileDocument {
        schema,
        profile_id: profile_id.ok_or(DecodeError::MissingField("profile_id"))?,
        kind: kind.ok_or(DecodeError::MissingField("kind"))?,
        display_name: display_name.ok_or(DecodeError::MissingField("display_name"))?,
        revision: revision.ok_or(DecodeError::MissingField("revision"))?,
        campaign,
        blueprints,
        records,
        settings,
        fingerprints,
        extra,
    };
    document.validate()?;
    Ok(document)
}
