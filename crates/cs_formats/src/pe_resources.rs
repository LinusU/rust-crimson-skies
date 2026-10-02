//! Bounded, cycle-checked reader for the Win32 resource section of a PE
//! image (`specs/F12-text-configuration-strings-and-pe-resources.md`, stage
//! `### F12-B`).
//!
//! The dialect inventory ([`crate::text::dialect::TextDialect::PeResources`])
//! records three surveyed images whose resource sections carry the game's
//! strings: `strings.dll`, `GOSDATA/ASSETS/BINARIES/language.dll` and
//! `GOSDATA/ASSETS/BINARIES/langui.dll`. This module reads their resource
//! **directory** as inert data. It never loads, maps, imports from or
//! executes a DLL: the crate has no dynamic-loading surface, the entrypoints
//! take a byte slice and return records, and a hostile offset is refused
//! before any bytes are handed out.
//!
//! # What the survey found
//!
//! A read-only structural survey of the three images (counts, ids, code pages
//! and sizes only, recorded in
//! `docs/findings/2026-09-29-f12-b-pe-resource-reader-and-typed-values.md`):
//! all three are PE32 (`magic 0x010b`, `machine 0x014c`) with a `.rsrc`
//! section. `strings.dll` carries three resource types at the top level —
//! `RT_STRING` (6), `RT_VERSION` (16) and an unassigned type `255`; the other
//! two carry `RT_STRING` alone. Every third-level id in all three images is
//! the language id `1033` (`0x0409`, en-US), with a recorded code page per
//! data entry (`1252` in `strings.dll`, `0` in the other two, which is the
//! resource compiler's "no code page" value and is not treated as a default).
//! Every observed string block holds a whole number of counted UTF-16LE
//! units.
//!
//! # Rules this reader follows
//!
//! Spec F12 non-negotiable #3: *resource directory offsets require bounds and
//! cycle checks, including language subtrees and code pages*. So:
//!
//! * every directory table, every 8-byte directory entry, every UTF-16 name
//!   and every 16-byte data entry is bounds-checked against the **resource
//!   directory's declared size**, not merely against the file, so a table
//!   that claims more entries than the section holds is refused;
//! * a leaf's data is located through the section table and checked against
//!   that section's *raw* bytes, so an RVA pointing into a section's
//!   uninitialised tail, into the headers, or outside every section yields no
//!   bytes at all;
//! * a directory already on the path being walked is refused as
//!   [`PeError::DirectoryCycle`] instead of being followed, and the walk is
//!   additionally bounded by the parse's independent recursion budget;
//! * the code page of every data entry is retained verbatim next to the
//!   language id, and never assumed to be the machine's code page.
//!
//! Nothing here is a measured original value: the layout constants are the
//! public PE/COFF format (`EvidenceClass::Documented`) and the survey rows
//! above are `ObservedTool`. Whether the shipped game reaches these strings
//! through the Win32 resource API, through its own
//! [`crate::text::resource_header`] table, or through both is **unknown** and
//! is recorded as such.
//!
//! # How the walk reads
//!
//! Every byte of every image is reached through [`Reader`]'s absolute-offset
//! windows ([`Reader::window`] and [`Reader::window_bytes`], stage F12-F), and
//! every little-endian word through that same reader's typed reads. The walk
//! is random-access by nature — a directory entry names where its table is —
//! so it keeps one reader over the whole image and opens a window at the
//! absolute offset each structure names, rather than through a module-private
//! set of accessors that would be a second implementation of the F03 bounded
//! reads: the checked extent, the byte-wise decoding, the [`ParseError`]
//! values anchored at the failing field's absolute offset and the refusal of
//! a hostile range are the primitives `io.rs` implements and
//! `specs/F03-bounded-binary-parsing-primitives.md` covers, used once.
//!
//! `u16_at` and `u32_at` are compositions of those primitives for the
//! header fields read one at a time; a whole record (a section header, a
//! directory header, a data entry) is read sequentially out of one window.

use std::fmt;

use crate::error::ParseError;
use crate::io::{AllocationBudget, ParseContext, Reader, RecursionBudget};

/// Entrypoint label both passes scope their errors with.
pub const PE_RESOURCES_ENTRYPOINT: &str = "pe.resources";

/// `IMAGE_DOS_SIGNATURE`.
pub const DOS_MAGIC: [u8; 2] = *b"MZ";

/// `IMAGE_NT_SIGNATURE`.
pub const PE_MAGIC: [u8; 4] = *b"PE\0\0";

/// Offset of `e_lfanew` inside the DOS header.
pub const DOS_LFANEW_OFFSET: u64 = 0x3c;

/// `IMAGE_FILE_HEADER` size.
pub const COFF_HEADER_BYTES: u64 = 20;

/// `IMAGE_SECTION_HEADER` size.
pub const SECTION_HEADER_BYTES: u64 = 40;

/// `IMAGE_OPTIONAL_HEADER32.Magic`.
pub const OPTIONAL_MAGIC_PE32: u16 = 0x010b;

/// `IMAGE_OPTIONAL_HEADER64.Magic`.
pub const OPTIONAL_MAGIC_PE32PLUS: u16 = 0x020b;

/// Offset of `SizeOfHeaders` from the start of the optional header; the same
/// in both optional-header shapes.
pub const SIZE_OF_HEADERS_OFFSET: u64 = 60;

/// Offset of the first data directory from the start of the optional header,
/// indexed by the optional-header shape: `[PE32, PE32+]`.
pub const DATA_DIRECTORIES_OFFSET: [u64; 2] = [96, 112];

/// `IMAGE_DIRECTORY_ENTRY_RESOURCE`'s index in the data-directory array.
pub const RESOURCE_DIRECTORY_INDEX: u32 = 2;

/// `LANG_NEUTRAL` (`0x0000`). Not observed in any surveyed image; the
/// constant names the value the format uses for a resource the resource
/// compiler gave no language. It is retained verbatim, never interpreted.
pub const LANG_NEUTRAL: u32 = 0x0000;

/// `LANG_ENGLISH_US` (`0x0409`, decimal 1033), the language id every
/// surveyed image records for every third-level entry.
pub const LANG_ENGLISH_US: u32 = 0x0409;

/// A second language the fixture's block 1 carries, so a test can pin that
/// two leaves under one block id are two strings rather than one overwritten
/// string. No surveyed image uses a third-level id of `1`, so the value is
/// the fixture's, not a measured one.
pub const LANG_ID_ONE: u32 = 1;

/// `IMAGE_DATA_DIRECTORY` size.
pub const DATA_DIRECTORY_BYTES: u64 = 8;

/// `IMAGE_RESOURCE_DIRECTORY` size.
pub const RESOURCE_DIRECTORY_HEADER_BYTES: u64 = 16;

/// `IMAGE_RESOURCE_DIRECTORY_ENTRY` size.
pub const RESOURCE_DIRECTORY_ENTRY_BYTES: u64 = 8;

/// `IMAGE_RESOURCE_DATA_ENTRY` size.
pub const RESOURCE_DATA_ENTRY_BYTES: u64 = 16;

/// The high bit of a name word and of an offset word: set means "a string
/// name follows" and "a subdirectory follows" respectively.
pub const HIGH_BIT: u32 = 0x8000_0000;

/// The offset that remains once [`HIGH_BIT`] is cleared.
pub const OFFSET_MASK: u32 = 0x7fff_ffff;

/// `RT_STRING`, the type whose leaves this crate decodes into string ids
/// (`Documented` Win32 resource type).
pub const RT_STRING: u32 = 6;

/// `RT_STRING` blocks hold sixteen string ids each (`Documented`).
pub const STRING_UNITS_PER_BLOCK: usize = 16;

/// The `id` a string carries: `(block - 1) * 16 + index` — the Win32
/// string-table numbering (`Documented` rule, measured directory entries).
///
/// Microsoft documents the rule in terms of the string *identifier*: "RC
/// allocates 16 strings per section and uses the identifier value to determine
/// which section is to contain the string. Strings whose identifiers differ
/// only in the bottom 4 bits are placed in the same section" (`STRINGTABLE
/// resource`,
/// <https://learn.microsoft.com/en-us/windows/win32/menurc/stringtable-resource>).
/// `block_id` is therefore the **directory-entry name**, not the string
/// identifier: `block_id * 16 + index` would treat the entry name as the
/// identifier and name a string one block (16) too high.
///
/// That the `RT_STRING` directory entries are counted from one — so entry `b`
/// holds identifiers `(b - 1) * 16 ..= (b - 1) * 16 + 15` — is **not** stated
/// by that page or by `LoadString`'s reference, so the entry numbering is
/// measured rather than documented. Task #368 measured that the
/// resource-compiler headers' contiguous identifier run `40000..=40170` is
/// stored under `langui.dll` sections `2501..=2511` — all present, each a full
/// sixteen units — while the zero-based `block_id * 16 + index` reading would
/// need section `2500`, which does not exist. Task #374 re-measured the same
/// image and recorded the contrary hypothesis and why it is rejected, in
/// `docs/findings/2026-10-02-t374-string-id-numbering.md`.
pub fn string_id(block_id: u16, index: u8) -> u32 {
    u32::from(block_id.saturating_sub(1)) * STRING_UNITS_PER_BLOCK as u32 + u32::from(index)
}

/// Why a PE image was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeError {
    /// A byte-range, encoding or budget failure from the checked reader.
    Parse(ParseError),
    /// The bytes are present but are not the structure the format requires.
    Malformed {
        /// Provenance label of the bytes.
        container: String,
        /// Absolute offset of the field that failed.
        offset: u64,
        /// The logical field.
        field: String,
        /// What the format requires there.
        expected: String,
        /// What the bytes hold.
        observed: String,
    },
    /// A directory, name, data or RVA offset leaves the table or the image
    /// that is supposed to hold it.
    OutsideTable {
        /// Provenance label of the bytes.
        container: String,
        /// Absolute offset of the field that failed.
        offset: u64,
        /// The logical field.
        field: String,
        /// The byte count the containing table or image offers.
        limit: u64,
        /// The byte count the entry needs.
        requested: u64,
    },
    /// A directory entry points at a directory already on the path being
    /// walked. Following it would never terminate.
    DirectoryCycle {
        /// Provenance label of the bytes.
        container: String,
        /// Absolute offset of the entry that closed the cycle.
        offset: u64,
        /// Absolute offset where that directory was first entered.
        first: u64,
    },
    /// A `RT_STRING` block is not shaped the way the format requires.
    StringBlock {
        /// Provenance label of the bytes.
        container: String,
        /// Absolute offset of the failure (inside the block, for a unit).
        offset: u64,
        /// The logical field.
        field: String,
        /// What the format requires there.
        expected: String,
        /// What the bytes hold.
        observed: String,
    },
}

impl PeError {
    /// Stable, machine-matchable label.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Parse(_) => "parse",
            Self::Malformed { .. } => "malformed",
            Self::OutsideTable { .. } => "outside_table",
            Self::DirectoryCycle { .. } => "directory_cycle",
            Self::StringBlock { .. } => "string_block",
        }
    }

    /// The container the bytes came from.
    pub fn container(&self) -> &str {
        match self {
            Self::Parse(error) => &error.container,
            Self::Malformed { container, .. }
            | Self::OutsideTable { container, .. }
            | Self::DirectoryCycle { container, .. }
            | Self::StringBlock { container, .. } => container,
        }
    }

    /// Absolute offset of the failure inside the container.
    pub fn offset(&self) -> u64 {
        match self {
            Self::Parse(error) => error.offset,
            Self::Malformed { offset, .. }
            | Self::OutsideTable { offset, .. }
            | Self::DirectoryCycle { offset, .. }
            | Self::StringBlock { offset, .. } => *offset,
        }
    }

    fn malformed(
        image: &Reader<'_>,
        offset: u64,
        field: &str,
        expected: String,
        observed: String,
    ) -> Self {
        Self::Malformed {
            container: image.container().to_owned(),
            offset,
            field: field.to_owned(),
            expected,
            observed,
        }
    }

    fn outside(image: &Reader<'_>, offset: u64, field: &str, limit: u64, requested: u64) -> Self {
        Self::OutsideTable {
            container: image.container().to_owned(),
            offset,
            field: field.to_owned(),
            limit,
            requested,
        }
    }

    fn string(
        image: &Reader<'_>,
        offset: u64,
        field: &str,
        expected: String,
        observed: String,
    ) -> Self {
        Self::StringBlock {
            container: image.container().to_owned(),
            offset,
            field: field.to_owned(),
            expected,
            observed,
        }
    }
}

impl fmt::Display for PeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => write!(f, "{error}"),
            Self::Malformed {
                container,
                offset,
                field,
                expected,
                observed,
            } => write!(
                f,
                "{container}: field `{field}` at offset {offset} is {observed}; expected \
                 {expected}"
            ),
            Self::OutsideTable {
                container,
                offset,
                field,
                limit,
                requested,
            } => write!(
                f,
                "{container}: field `{field}` at offset {offset} needs {requested} bytes of a \
                 {limit}-byte container"
            ),
            Self::DirectoryCycle {
                container,
                offset,
                first,
            } => write!(
                f,
                "{container}: the resource entry at offset {offset} points at the directory \
                 entered at offset {first} a second time"
            ),
            Self::StringBlock {
                container,
                offset,
                field,
                expected,
                observed,
            } => write!(
                f,
                "{container}: string block field `{field}` at offset {offset} is {observed}; \
                 expected {expected}"
            ),
        }
    }
}

impl std::error::Error for PeError {}

impl From<ParseError> for PeError {
    fn from(error: ParseError) -> Self {
        Self::Parse(error)
    }
}

/// One `IMAGE_SECTION_HEADER`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeSection {
    /// The section name without its NUL padding.
    pub name: String,
    /// `VirtualAddress`: the RVA this section starts at.
    pub virtual_address: u32,
    /// `VirtualSize`: the RVA span the section occupies in memory.
    pub virtual_size: u32,
    /// `SizeOfRawData`: the bytes the section has on disk.
    pub raw_size: u32,
    /// `PointerToRawData`: where those bytes start in the file.
    pub raw_pointer: u32,
}

/// One `IMAGE_DATA_DIRECTORY`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DataDirectory {
    /// `VirtualAddress` of the directory.
    pub virtual_address: u32,
    /// `Size` of the directory in bytes.
    pub size: u32,
}

/// Where a resource RVA lands in the file, and how many bytes follow it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RvaSpan {
    /// Offset of the RVA's first byte in the file.
    pub file_offset: u64,
    /// Bytes available from [`Self::file_offset`] to the end of the mapping
    /// (the section's raw bytes, or the image's headers).
    pub available: u64,
}

/// The header layout of one PE image: what the resource walk needs, and
/// nothing that would require mapping or executing the image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeLayout {
    container: String,
    image_len: u64,
    pe_offset: u32,
    machine: u16,
    optional_magic: u16,
    headers_size: u32,
    sections: Vec<PeSection>,
    resource: Option<DataDirectory>,
}

impl PeLayout {
    /// Provenance label of the bytes.
    pub fn container(&self) -> &str {
        &self.container
    }

    /// The image's byte length, the outer bound every range is checked
    /// against.
    pub fn image_len(&self) -> u64 {
        self.image_len
    }

    /// `e_lfanew`: where the `PE\0\0` signature and the COFF header start.
    pub fn pe_offset(&self) -> u32 {
        self.pe_offset
    }

    /// `IMAGE_FILE_HEADER.Machine`.
    pub fn machine(&self) -> u16 {
        self.machine
    }

    /// `IMAGE_OPTIONAL_HEADER.Magic`: [`OPTIONAL_MAGIC_PE32`] or
    /// [`OPTIONAL_MAGIC_PE32PLUS`].
    pub fn optional_magic(&self) -> u16 {
        self.optional_magic
    }

    /// `SizeOfHeaders`.
    pub fn headers_size(&self) -> u32 {
        self.headers_size
    }

    /// The section table, in image order.
    pub fn sections(&self) -> &[PeSection] {
        &self.sections
    }

    /// The resource data directory, or `None` when the image declares fewer
    /// than [`RESOURCE_DIRECTORY_INDEX`] + 1 data directories. An image
    /// without a resource section is a valid PE image, not a failure.
    pub fn resource_directory(&self) -> Option<DataDirectory> {
        self.resource
    }

    /// Translates `rva` into a file offset, without reading any bytes.
    ///
    /// Returns `None` when no mapping covers it. An RVA below the first
    /// section is the image's headers, which are mapped one to one; any other
    /// RVA must fall inside a section's `max(VirtualSize, SizeOfRawData)`
    /// span. The returned [`RvaSpan::available`] never runs past the end of
    /// the image's bytes and is `0` for an RVA that lands in a section's
    /// uninitialised tail, so a caller that checks its length against
    /// `available` cannot obtain a byte that is not there.
    pub fn map_rva(&self, rva: u32) -> Option<RvaSpan> {
        let first = self
            .sections
            .first()
            .map_or(0, |section| section.virtual_address);
        if u64::from(rva) < u64::from(first) {
            let offset = u64::from(rva);
            let headers = u64::from(self.headers_size).min(self.image_len);
            return Some(RvaSpan {
                file_offset: offset,
                available: headers.saturating_sub(offset),
            });
        }
        for section in &self.sections {
            let span = u64::from(section.virtual_size.max(section.raw_size));
            if u64::from(rva) < u64::from(section.virtual_address) + span {
                let delta = u64::from(rva) - u64::from(section.virtual_address);
                let available = u64::from(section.raw_size).saturating_sub(delta);
                let file_offset = u64::from(section.raw_pointer) + delta;
                if file_offset > self.image_len {
                    return None;
                }
                return Some(RvaSpan {
                    file_offset,
                    available: available.min(self.image_len - file_offset),
                });
            }
        }
        None
    }
}

/// The four numbers of one `IMAGE_SECTION_HEADER` the RVA translation needs.
///
/// A `SectionSpan` is read on demand from the image rather than collected into
/// a table, so the structure pass can translate an RVA without allocating.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SectionSpan {
    virtual_address: u32,
    virtual_size: u32,
    raw_size: u32,
    raw_pointer: u32,
}

impl SectionSpan {
    /// Where the section's raw bytes are in the file, and how many follow.
    fn span(&self, rva: u32, image_len: u64) -> Option<RvaSpan> {
        let reach = u64::from(self.virtual_size.max(self.raw_size));
        if u64::from(rva) < u64::from(self.virtual_address)
            || u64::from(rva) >= u64::from(self.virtual_address) + reach
        {
            return None;
        }
        let delta = u64::from(rva) - u64::from(self.virtual_address);
        let available = u64::from(self.raw_size).saturating_sub(delta);
        let file_offset = u64::from(self.raw_pointer).saturating_add(delta);
        if file_offset > image_len {
            return None;
        }
        Some(RvaSpan {
            file_offset,
            available: available.min(image_len - file_offset),
        })
    }
}

/// The header fields the structure pass needs from a PE image, read without
/// allocating a section table.
struct LayoutFacts {
    pe_offset: u32,
    headers_size: u32,
    sections_at: u64,
    section_count: u16,
    resource: Option<DataDirectory>,
}

/// Reads the header fields once, checking the same things [`read_layout`]
/// checks, and books nothing.
fn read_layout_facts(image: &Reader<'_>) -> Result<LayoutFacts, PeError> {
    let facts = read_header_fields(image)?;
    // The section table itself has to be inside the image before any of its
    // rows can be read.
    image.window_bytes(
        facts.sections_at,
        u64::from(facts.section_count) * SECTION_HEADER_BYTES,
        "pe.sections",
    )?;
    Ok(facts)
}

/// The three identity fields [`PeLayout`] reports, read on their own so both
/// passes can take them without duplicating the COFF-header walk.
struct PeLayoutFields {
    pe_offset: u32,
    machine: u16,
    optional_magic: u16,
}

fn read_pe_offset(image: &Reader<'_>) -> Result<PeLayoutFields, PeError> {
    let pe_offset = read_header_fields(image)?.pe_offset;
    let coff = u64::from(pe_offset) + 4;
    Ok(PeLayoutFields {
        pe_offset,
        machine: u16_at(image, coff, "pe.coff.machine")?,
        optional_magic: u16_at(image, coff + COFF_HEADER_BYTES, "pe.optional.magic")?,
    })
}

/// The header fields shared by the allocation-free pass and
/// [`read_layout`].
fn read_header_fields(image: &Reader<'_>) -> Result<LayoutFacts, PeError> {
    let dos_magic = image.window_bytes(0, 2, "pe.dos.magic")?;
    if dos_magic != DOS_MAGIC {
        return Err(PeError::malformed(
            image,
            0,
            "pe.dos.magic",
            "the DOS signature MZ".to_owned(),
            format!("{dos_magic:02x?}"),
        ));
    }
    let pe_offset = u32_at(image, DOS_LFANEW_OFFSET, "pe.dos.e_lfanew")?;
    let signature = u64::from(pe_offset);
    let nt_signature = image.window_bytes(signature, 4, "pe.signature")?;
    if nt_signature != PE_MAGIC {
        return Err(PeError::malformed(
            image,
            signature,
            "pe.signature",
            "the NT signature PE\\0\\0".to_owned(),
            format!("{nt_signature:02x?}"),
        ));
    }
    let coff = signature + 4;
    let section_count = u16_at(image, coff + 2, "pe.coff.number_of_sections")?;
    let optional_size = u64::from(u16_at(image, coff + 16, "pe.coff.size_of_optional_header")?);
    let optional = coff + COFF_HEADER_BYTES;
    let magic = u16_at(image, optional, "pe.optional.magic")?;
    let directories = match magic {
        OPTIONAL_MAGIC_PE32 => DATA_DIRECTORIES_OFFSET[0],
        OPTIONAL_MAGIC_PE32PLUS => DATA_DIRECTORIES_OFFSET[1],
        other => {
            return Err(PeError::malformed(
                image,
                optional,
                "pe.optional.magic",
                "0x010b (PE32) or 0x020b (PE32+)".to_owned(),
                format!("0x{other:04x}"),
            ));
        }
    };
    let headers_size = u32_at(
        image,
        optional + SIZE_OF_HEADERS_OFFSET,
        "pe.optional.size_of_headers",
    )?;
    // `NumberOfRvaAndSizes` is the four bytes immediately before the first
    // `IMAGE_DATA_DIRECTORY` (offset 92 in PE32, 108 in PE32+).
    let directory_count = u32_at(
        image,
        optional + directories - 4,
        "pe.optional.number_of_rva_and_sizes",
    )?;
    let sections_at = optional.checked_add(optional_size).ok_or_else(|| {
        ParseError::length_overflow(
            image.container().to_owned(),
            optional,
            "pe.coff.size_of_optional_header",
            "the optional header end to fit in u64".to_owned(),
            format!("{optional} plus {optional_size}"),
        )
    })?;
    let resource = if directory_count > RESOURCE_DIRECTORY_INDEX {
        let at =
            optional + directories + u64::from(RESOURCE_DIRECTORY_INDEX) * DATA_DIRECTORY_BYTES;
        Some(DataDirectory {
            virtual_address: u32_at(image, at, "pe.directory.resource.virtual_address")?,
            size: u32_at(image, at + 4, "pe.directory.resource.size")?,
        })
    } else {
        None
    };
    Ok(LayoutFacts {
        pe_offset,
        headers_size,
        sections_at,
        section_count,
        resource,
    })
}

/// One section header row, read on demand.
///
/// The four numbers the RVA translation needs are the second half of the
/// 40-byte record, so one window over the record's tail is read forward
/// through [`Reader`]'s typed reads rather than four windows over four
/// fields.
fn section_span(image: &Reader<'_>, sections_at: u64, index: u16) -> Result<SectionSpan, PeError> {
    let at = sections_at + u64::from(index) * SECTION_HEADER_BYTES + 8;
    let mut row = image.window(at, 16, "pe.section")?;
    Ok(SectionSpan {
        virtual_size: row.read_u32("pe.section.virtual_size")?,
        virtual_address: row.read_u32("pe.section.virtual_address")?,
        raw_size: row.read_u32("pe.section.size_of_raw_data")?,
        raw_pointer: row.read_u32("pe.section.pointer_to_raw_data")?,
    })
}

/// Translates an RVA with no section table collected: the headers are mapped
/// one to one below the first section, and any other RVA must fall inside a
/// section's `max(VirtualSize, SizeOfRawData)` span.
fn map_rva(image: &Reader<'_>, facts: &LayoutFacts, rva: u32) -> Result<Option<RvaSpan>, PeError> {
    let mut first = None;
    let mut span_for = None;
    for index in 0..facts.section_count {
        let section = section_span(image, facts.sections_at, index)?;
        first.get_or_insert(section.virtual_address);
        if let Some(span) = section.span(rva, image.range_end()) {
            span_for = Some(span);
            break;
        }
    }
    if let Some(span) = span_for {
        return Ok(Some(span));
    }
    // No section covers it. Below the first section's RVA it is one of the
    // image's headers, which are mapped one to one.
    let first = u64::from(first.unwrap_or(0));
    if u64::from(rva) < first {
        let offset = u64::from(rva);
        let headers = u64::from(facts.headers_size).min(image.range_end());
        return Ok(Some(RvaSpan {
            file_offset: offset,
            available: headers.saturating_sub(offset),
        }));
    }
    Ok(None)
}

/// One resource tree key: a numeric id, or a UTF-16 name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResourceKey {
    /// A numeric id, as recorded in the entry's name word.
    Id(u32),
    /// A name, kept as its exact UTF-16 code units so that no encoding is
    /// assumed. The surveyed images use ids at every level; the name branch
    /// exists because the format allows it.
    Name(Vec<u16>),
}

impl ResourceKey {
    /// The numeric id, or `None` for a name.
    pub fn id(&self) -> Option<u32> {
        match self {
            Self::Id(id) => Some(*id),
            Self::Name(_) => None,
        }
    }

    /// The name's code units, or `None` for an id.
    pub fn code_units(&self) -> Option<&[u16]> {
        match self {
            Self::Id(_) => None,
            Self::Name(units) => Some(units),
        }
    }

    /// The name as text, or `None` when the entry is an id or the name holds
    /// an unpaired surrogate (recorded, never replaced).
    pub fn text(&self) -> Option<String> {
        match self {
            Self::Id(_) => None,
            Self::Name(units) => String::from_utf16(units).ok(),
        }
    }

    /// Stable label of the key's kind.
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Id(_) => "id",
            Self::Name(_) => "name",
        }
    }

    /// Whether the key is a name rather than a numeric id.
    pub const fn is_name(&self) -> bool {
        matches!(self, Self::Name(_))
    }
}

/// One `IMAGE_RESOURCE_DATA_ENTRY` with its RVA resolved to a file offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceData {
    /// Offset of the data entry inside the resource directory.
    pub directory_offset: u32,
    /// The entry's `OffsetToData` RVA, verbatim.
    pub rva: u32,
    /// The file offset that RVA maps to.
    pub file_offset: u64,
    /// `Size`: the leaf's extent in bytes.
    pub size: u32,
    /// `CodePage`, verbatim. `0` is the resource compiler's "no code page"
    /// value and is **not** treated as a default.
    pub code_page: u32,
    /// `Reserved`, verbatim.
    pub reserved: u32,
}

/// One leaf of the resource tree: the keys from the root down to it, and the
/// data it points at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceLeaf {
    /// The keys, outermost first. The surveyed images use three levels
    /// (type, name or id, language); the format does not fix the depth, so
    /// the path is kept as written instead of being padded or truncated.
    pub path: Vec<ResourceKey>,
    /// The leaf's data entry.
    pub data: ResourceData,
}

impl ResourceLeaf {
    /// The key at `depth` (0 is the resource type).
    pub fn key(&self, depth: usize) -> Option<&ResourceKey> {
        self.path.get(depth)
    }
}

/// One counted UTF-16 string of a `RT_STRING` block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StringUnit {
    /// Position inside its block, `0..16`.
    pub index: u8,
    /// The string id this unit carries: [`string_id`] of its block and index.
    pub id: u32,
    /// The exact code units, kept even when they are not valid text.
    pub code_units: Vec<u16>,
    /// The text, or `None` when the units hold an unpaired surrogate. A
    /// missing text is a recorded condition, never a replaced one.
    pub text: Option<String>,
}

/// One decoded `RT_STRING` block: sixteen string ids' worth of data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StringBlock {
    /// The second-level id, which selects the block: unit `i` of block `b`
    /// carries id `(b - 1) * 16 + i`.
    pub block_id: u16,
    /// The third-level language id, verbatim.
    pub language: u32,
    /// The data entry's code page, verbatim.
    pub code_page: u32,
    /// The data entry the block was read from.
    pub data: ResourceData,
    /// The units, in block order. Fewer than [`STRING_UNITS_PER_BLOCK`]
    /// means the block ended early.
    pub units: Vec<StringUnit>,
    /// Bytes left after the last complete unit. A whole number of units is
    /// the norm; trailing bytes are counted, never skipped silently.
    pub trailing_bytes: u32,
}

/// Everything this crate reads out of one PE image's resource section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeResources {
    layout: PeLayout,
    directories: u32,
    leaves: Vec<ResourceLeaf>,
    strings: Vec<StringBlock>,
}

impl PeResources {
    /// The image's header layout.
    pub fn layout(&self) -> &PeLayout {
        &self.layout
    }

    /// How many resource directories the walk entered.
    pub fn directories(&self) -> u32 {
        self.directories
    }

    /// Every leaf, in the order the walk reached them.
    pub fn leaves(&self) -> &[ResourceLeaf] {
        &self.leaves
    }

    /// The decoded `RT_STRING` blocks, in walk order.
    pub fn strings(&self) -> &[StringBlock] {
        &self.strings
    }

    /// The `RT_STRING` block with second-level id `block_id`, or `None`.
    pub fn string_block(&self, block_id: u16) -> Option<&StringBlock> {
        self.strings.iter().find(|block| block.block_id == block_id)
    }

    /// The one string with id `id` in `language` (any language when `None`).
    ///
    /// Two leaves carrying the same id under different languages are *not*
    /// collapsed: a caller that wants one language asks for it.
    pub fn string_unit(&self, id: u32, language: Option<u32>) -> Option<&StringUnit> {
        self.strings
            .iter()
            .filter(|block| language.is_none_or(|wanted| block.language == wanted))
            .flat_map(|block| &block.units)
            .find(|unit| unit.id == id)
    }
}

/// One little-endian `u16` of the image at the absolute offset `at`.
///
/// A composition of the two shared primitives and nothing else:
/// [`Reader::window`] bounds-checks the two bytes against the image and
/// [`Reader::read_u16`] decodes them, so a hostile `at` costs the
/// [`ParseError`] the same primitive builds everywhere else and this module
/// decodes no bytes itself.
fn u16_at(image: &Reader<'_>, at: u64, field: &str) -> Result<u16, PeError> {
    let mut window = image.window(at, 2, field)?;
    Ok(window.read_u16(field)?)
}

/// One little-endian `u32` of the image at the absolute offset `at`.
///
/// See [`u16_at`] for what this composes and does not.
fn u32_at(image: &Reader<'_>, at: u64, field: &str) -> Result<u32, PeError> {
    let mut window = image.window(at, 4, field)?;
    Ok(window.read_u32(field)?)
}

/// A key of the walk, still undecoded: an id, or the offset of a name string
/// inside the resource directory. Keeping it undecoded is what lets the
/// structure pass walk the tree without allocating a name per entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeyRef {
    Id(u32),
    Name { at: u32, units: u32 },
}

/// The resource-directory walk over one image.
///
/// The walk reads its section headers on demand through the `image` it was
/// given, so the structure pass can run it without booking a section table.
struct Walker<'i, 'b> {
    image: &'i Reader<'b>,
    facts: &'i LayoutFacts,
    /// File offset of the resource directory.
    base: u64,
    /// Its declared size: every offset is checked against this, not against
    /// the image.
    size: u64,
    directories: u32,
}

impl<'i, 'b> Walker<'i, 'b> {
    fn new(
        image: &'i Reader<'b>,
        facts: &'i LayoutFacts,
        directory: DataDirectory,
    ) -> Result<Self, PeError> {
        if u64::from(directory.size) < RESOURCE_DIRECTORY_HEADER_BYTES {
            return Err(PeError::outside(
                image,
                u64::from(directory.virtual_address),
                "pe.directory.resource.size",
                RESOURCE_DIRECTORY_HEADER_BYTES,
                u64::from(directory.size),
            ));
        }
        let span = map_rva(image, facts, directory.virtual_address)?.ok_or_else(|| {
            PeError::outside(
                image,
                u64::from(directory.virtual_address),
                "pe.directory.resource.virtual_address",
                image.range_end(),
                u64::from(directory.size),
            )
        })?;
        if u64::from(directory.size) > span.available {
            return Err(PeError::outside(
                image,
                span.file_offset,
                "pe.directory.resource.size",
                span.available,
                u64::from(directory.size),
            ));
        }
        Ok(Self {
            image,
            facts,
            base: span.file_offset,
            size: u64::from(directory.size),
            directories: 0,
        })
    }

    /// The absolute file offset of `[rel, rel + len)`, after checking that the
    /// range lies inside the resource directory.
    ///
    /// This is the *directory's* bound, not the image's: every offset in the
    /// tree is checked against the size the data directory declares (spec F12
    /// non-negotiable #3), so a table that claims more than the section holds
    /// is refused here even when the image itself is longer. The image's own
    /// bound is the second half of the check, in [`Reader::window`], which
    /// every window this walk opens goes through.
    fn check_table(&self, rel: u32, len: u64, field: &str) -> Result<u64, PeError> {
        let offset = u64::from(rel);
        let end = offset.checked_add(len).ok_or_else(|| {
            ParseError::length_overflow(
                self.image.container().to_owned(),
                self.base + offset,
                field,
                "offset + length to fit in u64".to_owned(),
                format!("offset {offset} plus length {len}"),
            )
        })?;
        if end > self.size {
            return Err(PeError::outside(
                self.image,
                self.base + offset,
                field,
                self.size,
                end,
            ));
        }
        Ok(self.base + offset)
    }

    /// A reader over `[rel, rel + len)` of the resource directory.
    fn table(&self, rel: u32, len: u64, field: &str) -> Result<Reader<'b>, PeError> {
        let at = self.check_table(rel, len, field)?;
        Ok(self.image.window(at, len, field)?)
    }

    /// The bytes of `[rel, rel + len)` of the resource directory, borrowed
    /// from the image rather than copied.
    fn table_bytes(&self, rel: u32, len: u64, field: &str) -> Result<&'b [u8], PeError> {
        let at = self.check_table(rel, len, field)?;
        Ok(self.image.window_bytes(at, len, field)?)
    }

    /// One directory name word, decoded without allocating.
    fn key(&self, word: u32) -> Result<KeyRef, PeError> {
        if word & HIGH_BIT == 0 {
            return Ok(KeyRef::Id(word));
        }
        let at = word & OFFSET_MASK;
        // The count and the code units it announces both have to be inside
        // the directory, so a name that reaches past the section is refused
        // before a single unit is read.
        let mut length_word = self.table(at, 2, "resources.name.length")?;
        let length = u64::from(length_word.read_u16("resources.name.length")?);
        self.table_bytes(at, 2 + length * 2, "resources.name.code_units")?;
        Ok(KeyRef::Name {
            at,
            units: u32::try_from(length).expect("a u16 count fits in a u32"),
        })
    }

    /// One `IMAGE_RESOURCE_DATA_ENTRY`, read forward out of a single window.
    fn data(&self, rel: u32) -> Result<ResourceData, PeError> {
        let at = u64::from(rel);
        let mut entry = self.table(rel, RESOURCE_DATA_ENTRY_BYTES, "resources.data")?;
        let rva = entry.read_u32("resources.data.rva")?;
        let size = entry.read_u32("resources.data.size")?;
        let code_page = entry.read_u32("resources.data.code_page")?;
        let reserved = entry.read_u32("resources.data.reserved")?;
        let span = map_rva(self.image, self.facts, rva)?.ok_or_else(|| {
            PeError::outside(
                self.image,
                self.base + at,
                "resources.data.rva",
                self.image.range_end(),
                u64::from(size),
            )
        })?;
        if u64::from(size) > span.available {
            return Err(PeError::outside(
                self.image,
                span.file_offset,
                "resources.data.size",
                span.available,
                u64::from(size),
            ));
        }
        Ok(ResourceData {
            directory_offset: rel,
            rva,
            file_offset: span.file_offset,
            size,
            code_page,
            reserved,
        })
    }

    /// Walks the tree from the root, handing every leaf to `leaf`.
    ///
    /// The cycle check runs *before* the directory is read, so a directory
    /// that is already on the path being walked is refused rather than
    /// followed (spec F12 non-negotiable #3), and the nesting is bounded twice
    /// over: by the parse's [`RecursionBudget`] and by
    /// [`MAX_RESOURCE_DEPTH`].
    fn walk(
        &mut self,
        rel: u32,
        path: &mut WalkPath,
        recursion: &RecursionBudget,
        leaf: &mut impl FnMut(&[KeyRef], &ResourceData) -> Result<(), PeError>,
    ) -> Result<(), PeError> {
        if let Some(first) = path.ancestors().iter().position(|seen| *seen == rel) {
            return Err(PeError::DirectoryCycle {
                container: self.image.container().to_owned(),
                offset: self.base + u64::from(rel),
                first: self.base + u64::from(path.ancestors()[first]),
            });
        }
        let _budget = recursion.enter("resources.directory", self.base + u64::from(rel))?;
        path.enter(rel, self.image.container())?;
        self.directories += 1;

        // The directory header and the entry table that follows it are one
        // contiguous table, so one window covers both: the counts come out of
        // the header, and the entries are then read forward, two words at a
        // time, exactly as they sit in the format.
        let mut table = self.table(rel, RESOURCE_DIRECTORY_HEADER_BYTES, "resources.directory")?;
        // `Characteristics`, `TimeDateStamp`, `MajorVersion` and
        // `MinorVersion` are recorded by the format and interpreted by nothing
        // here, so the walk steps over them rather than inventing meanings.
        table.skip("resources.directory.uninterpreted_header", 12)?;
        let named = table.read_u16("resources.directory.number_of_named_entries")?;
        let ids = table.read_u16("resources.directory.number_of_id_entries")?;
        let count = u64::from(named) + u64::from(ids);
        let table_bytes = RESOURCE_DIRECTORY_HEADER_BYTES
            .checked_add(count * RESOURCE_DIRECTORY_ENTRY_BYTES)
            .ok_or_else(|| {
                ParseError::length_overflow(
                    self.image.container().to_owned(),
                    self.base + u64::from(rel),
                    "resources.directory.entries",
                    "directory table size to fit in u64".to_owned(),
                    format!("{count} entries"),
                )
            })?;
        let mut entries = self.table(rel, table_bytes, "resources.directory.entries")?;
        entries.skip(
            "resources.directory.header",
            RESOURCE_DIRECTORY_HEADER_BYTES as usize,
        )?;

        for _ in 0..count {
            let name_word = entries.read_u32("resources.directory.entries.name")?;
            let target_word = entries.read_u32("resources.directory.entries.target")?;
            path.push_key(self.key(name_word)?);
            if target_word & HIGH_BIT == 0 {
                let data = self.data(target_word)?;
                leaf(path.keys(), &data)?;
            } else {
                self.walk(target_word & OFFSET_MASK, path, recursion, leaf)?;
            }
            path.clear_key();
        }
        path.leave();
        Ok(())
    }
}

/// The nesting ceiling the walk holds on the stack, and so the deepest
/// resource tree [`read_pe_resources`] reads.
///
/// A `Designed` value (EvidenceClass `Designed`), like
/// [`RecursionBudget::DEFAULT_MAX_DEPTH`]: the three surveyed images nest three
/// levels, and a walk that would nest deeper than this is refused as
/// [`crate::ParseErrorKind::RecursionDepthExceeded`] rather than growing a vector. A
/// caller that genuinely needs more may raise
/// [`RecursionBudget::max_depth`], but only up to this ceiling, so the walk's
/// own memory stays bounded independently of the parse's budgets.
pub const MAX_RESOURCE_DEPTH: usize = 32;

/// The two stacks a walk keeps: the keys of the path being walked, and the
/// directory offsets already on it (the cycle set).
///
/// Both live on the walk's stack frame, so a walk allocates nothing: the
/// structure pass can therefore refuse a hostile image without having to roll
/// anything back, and the build pass books only the records it hands out.
struct WalkPath {
    keys: [KeyRef; MAX_RESOURCE_DEPTH],
    ancestors: [u32; MAX_RESOURCE_DEPTH],
    depth: usize,
}

impl WalkPath {
    fn new() -> Self {
        Self {
            keys: [KeyRef::Id(0); MAX_RESOURCE_DEPTH],
            ancestors: [0; MAX_RESOURCE_DEPTH],
            depth: 0,
        }
    }

    /// The keys of the path being walked, outermost first.
    fn keys(&self) -> &[KeyRef] {
        &self.keys[..self.depth]
    }

    /// The directory offsets on the path, the last one being the directory
    /// currently being read.
    fn ancestors(&self) -> &[u32] {
        &self.ancestors[..self.depth]
    }

    /// Descends into `rel`, or refuses when the ceiling is reached.
    ///
    /// The level is released by [`Self::leave`]. A refusal aborts the whole
    /// walk, so an error propagating out leaves the frame's depth where it
    /// was: the frame is dropped with the walk, never reused.
    fn enter(&mut self, rel: u32, container: &str) -> Result<(), PeError> {
        if self.depth >= MAX_RESOURCE_DEPTH {
            return Err(ParseError::recursion_depth_exceeded(
                container.to_owned(),
                u64::from(rel),
                "resources.directory",
                MAX_RESOURCE_DEPTH as u32,
                self.depth as u64 + 1,
            )
            .into());
        }
        self.ancestors[self.depth] = rel;
        self.depth += 1;
        Ok(())
    }

    /// Releases the level [`Self::enter`] took.
    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    /// Records the key of the level just entered.
    fn push_key(&mut self, key: KeyRef) {
        self.keys[self.depth - 1] = key;
    }

    /// Clears the key of the current level, so a later level never reads a
    /// stale one.
    fn clear_key(&mut self) {
        self.keys[self.depth - 1] = KeyRef::Id(0);
    }
}

/// Runs `attempt` inside one `pe.resources` entrypoint, so both budgets apply,
/// a failed attempt rolls its charges back and a byte-level failure keeps the
/// `ParseError`'s field path.
///
/// A domain refusal ([`PeError::Malformed`], [`PeError::OutsideTable`],
/// [`PeError::DirectoryCycle`], [`PeError::StringBlock`]) is *not* a
/// [`ParseError`] and cannot cross [`ParseContext::parse`]; it is carried
/// through as a nested `Result` the way the ROF reader carries its own. The
/// callers below therefore keep the structure pass allocation-free apart from
/// the section table and the two path vectors, so an unrolled domain refusal
/// still leaves a correct ledger.
fn scoped<T>(
    context: &mut ParseContext,
    bytes: &[u8],
    attempt: impl FnOnce(&mut AllocationBudget, &RecursionBudget) -> Result<T, PeError>,
) -> Result<T, PeError> {
    match context.parse(
        PE_RESOURCES_ENTRYPOINT,
        bytes,
        |_reader, allocation, recursion| match attempt(allocation, recursion) {
            Ok(value) => Ok(Ok(value)),
            Err(PeError::Parse(error)) => Err(error),
            Err(domain) => Ok(Err(domain)),
        },
    ) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(domain)) => Err(domain),
        Err(error) => Err(PeError::Parse(error)),
    }
}

/// Reads the header layout of the PE image in `bytes`.
///
/// Only the fields the resource walk needs are decoded; no section is mapped,
/// no import is followed and no entry point is read. The section table is the
/// one buffer this allocates and it is booked before it is built.
///
/// # Errors
///
/// [`PeError::Malformed`] when the bytes are not a PE image this reader
/// understands (no `MZ`, no `PE\0\0`, an optional-header shape it does not
/// know), and [`PeError::Parse`] when a read runs past the image or the
/// section table does not fit the parse's allocation budget.
pub fn read_pe_layout(context: &mut ParseContext, bytes: &[u8]) -> Result<PeLayout, PeError> {
    let image = Reader::new(context.container(), bytes);
    scoped(context, bytes, |allocation, _recursion| {
        read_layout(&image, allocation)
    })
}

fn read_layout(image: &Reader<'_>, allocation: &mut AllocationBudget) -> Result<PeLayout, PeError> {
    let facts = read_layout_facts(image)?;
    let PeLayoutFields {
        pe_offset,
        machine,
        optional_magic,
        ..
    } = read_pe_offset(image)?;

    // The section table is the one buffer this function allocates, so it is
    // booked (and can be refused) before it is built.
    let count = u64::from(facts.section_count);
    allocation.reserve("sections", facts.sections_at, count, SECTION_HEADER_BYTES)?;
    let mut sections = Vec::with_capacity(facts.section_count as usize);
    for index in 0..facts.section_count as usize {
        // One window per 40-byte `IMAGE_SECTION_HEADER`, read forward: the
        // eight name bytes, then the four numbers the RVA translation needs.
        let at = facts.sections_at + index as u64 * SECTION_HEADER_BYTES;
        let mut row = image.window(at, SECTION_HEADER_BYTES, "pe.section")?;
        let name_bytes: [u8; 8] = row
            .read_bytes("pe.section.name", 8)?
            .try_into()
            .expect("an 8-byte name field");
        let end = name_bytes
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(name_bytes.len());
        let name = String::from_utf8_lossy(&name_bytes[..end]).into_owned();
        sections.push(PeSection {
            name,
            virtual_size: row.read_u32("pe.section.virtual_size")?,
            virtual_address: row.read_u32("pe.section.virtual_address")?,
            raw_size: row.read_u32("pe.section.size_of_raw_data")?,
            raw_pointer: row.read_u32("pe.section.pointer_to_raw_data")?,
        });
    }

    Ok(PeLayout {
        container: image.container().to_owned(),
        // The image reader spans the whole input, so its range end is the
        // image's byte length.
        image_len: image.range_end(),
        pe_offset,
        machine,
        optional_magic,
        headers_size: facts.headers_size,
        sections,
        resource: facts.resource,
    })
}

/// Reads the resource section of the PE image in `bytes`.
///
/// # Errors
///
/// Everything [`read_pe_layout`] reports, plus [`PeError::OutsideTable`] for
/// a directory, name or data offset that leaves the resource directory's
/// declared size, and again for a leaf whose data does not fit its section's
/// raw bytes; [`PeError::DirectoryCycle`] for a directory already on the path
/// being walked; and [`PeError::StringBlock`] for a `RT_STRING` block that is
/// not shaped the way the format requires.
///
/// The walk runs in two passes over the same immutable bytes. The first
/// validates the whole structure and allocates only the section table and the
/// two small path vectors, so a malformed image is refused with almost
/// nothing booked; the second books and builds the records. A structural
/// failure therefore cannot appear in the second pass, and the build tests
/// exercise that pass's structural refusals directly.
pub fn read_pe_resources(context: &mut ParseContext, bytes: &[u8]) -> Result<PeResources, PeError> {
    let image = Reader::new(context.container(), bytes);
    // Pass one: the whole structure. It allocates nothing — the section
    // headers and the two walk stacks are read from the image and the frame —
    // so a hostile image is refused with the ledger exactly as it was, and
    // every offset the build pass will use has already been checked.
    let facts = scoped(context, bytes, |_allocation, recursion| {
        let facts = read_layout_facts(&image)?;
        if let Some(directory) = facts.resource {
            let mut walker = Walker::new(&image, &facts, directory)?;
            let mut path = WalkPath::new();
            walker.walk(0, &mut path, recursion, &mut |keys, data| {
                check_leaf(&image, keys, data)
            })?;
        }
        Ok(facts)
    })?;

    // Pass two: the records, every buffer booked. The structure pass proved
    // every offset, cycle and string block first, so the only failure left
    // here is a budget refusal, which the entrypoint rolls back.
    scoped(context, bytes, |allocation, recursion| {
        let layout = read_layout(&image, allocation)?;
        build(&image, &layout, &facts, allocation, recursion)
    })
}

/// The structure pass's per-leaf check: only a three-level `RT_STRING` leaf
/// whose second level is a block id the format can number is a string block,
/// and its counted units must fit its own extent. Every other leaf is a plain
/// leaf and is not interpreted here.
fn check_leaf(image: &Reader<'_>, keys: &[KeyRef], data: &ResourceData) -> Result<(), PeError> {
    if let Some((block, _)) = string_leaf(keys) {
        check_block_id(image, data, block)?;
        return check_string_block(image, data);
    }
    Ok(())
}

/// The block id and language id of a `RT_STRING` leaf, or `None` for a leaf
/// the reader does not interpret as a string block.
fn string_leaf(keys: &[KeyRef]) -> Option<(u32, u32)> {
    match keys {
        [
            KeyRef::Id(RT_STRING),
            KeyRef::Id(block),
            KeyRef::Id(language),
        ] => Some((*block, *language)),
        _ => None,
    }
}

/// Refuses a block id the format cannot number a string with: `0`, and
/// anything wider than the 16 bits a second-level id has.
fn check_block_id(image: &Reader<'_>, data: &ResourceData, block: u32) -> Result<u16, PeError> {
    let id = u16::try_from(block).map_err(|_| {
        PeError::string(
            image,
            data.file_offset,
            "block.id",
            "a second-level id that fits in 16 bits".to_owned(),
            block.to_string(),
        )
    })?;
    if id == 0 {
        return Err(PeError::string(
            image,
            data.file_offset,
            "block.id",
            "a block id of at least 1 (ids number (block - 1) * 16 + index)".to_owned(),
            "0".to_owned(),
        ));
    }
    Ok(id)
}

/// Refuses a `RT_STRING` block whose counted units leave its own extent.
/// Allocation free, so the structure pass runs it on every string block.
fn check_string_block(image: &Reader<'_>, data: &ResourceData) -> Result<(), PeError> {
    let bytes = image.window_bytes(
        data.file_offset,
        u64::from(data.size),
        "resources.string_block",
    )?;
    let mut at = 0usize;
    for _ in 0..STRING_UNITS_PER_BLOCK {
        if at == bytes.len() {
            return Ok(());
        }
        let Some(length_bytes) = bytes.get(at..at + 2) else {
            return Err(PeError::string(
                image,
                data.file_offset + at as u64,
                "unit.length",
                "2 bytes of length".to_owned(),
                format!("{} bytes left", bytes.len() - at),
            ));
        };
        let length = usize::from(u16::from_le_bytes([length_bytes[0], length_bytes[1]]));
        let Some(units) = bytes.get(at + 2..at + 2 + length * 2) else {
            return Err(PeError::string(
                image,
                data.file_offset + at as u64,
                "unit.code_units",
                format!("{} bytes of code units", length * 2),
                format!("{} bytes left", bytes.len() - at - 2),
            ));
        };
        let _ = units;
        at += 2 + length * 2;
    }
    Ok(())
}

/// Builds every record of one image.
fn build(
    image: &Reader<'_>,
    layout: &PeLayout,
    facts: &LayoutFacts,
    allocation: &mut AllocationBudget,
    recursion: &RecursionBudget,
) -> Result<PeResources, PeError> {
    let mut resources = PeResources {
        layout: layout.clone(),
        directories: 0,
        leaves: Vec::new(),
        strings: Vec::new(),
    };
    let Some(directory) = layout.resource_directory() else {
        return Ok(resources);
    };
    let mut walker = Walker::new(image, facts, directory)?;
    let base = walker.base;
    let mut path = WalkPath::new();
    walker.walk(0, &mut path, recursion, &mut |keys, data| {
        build_leaf(&mut resources, image, base, keys, data, allocation)
    })?;
    resources.directories = walker.directories;
    Ok(resources)
}

/// Turns one walked leaf into a record, booking every buffer it owns.
fn build_leaf(
    resources: &mut PeResources,
    image: &Reader<'_>,
    base: u64,
    keys: &[KeyRef],
    data: &ResourceData,
    allocation: &mut AllocationBudget,
) -> Result<(), PeError> {
    allocation.reserve(
        "resource_key",
        data.file_offset,
        keys.len() as u64,
        std::mem::size_of::<ResourceKey>() as u64,
    )?;
    let mut path = Vec::with_capacity(keys.len());
    for key in keys {
        path.push(match *key {
            KeyRef::Id(id) => ResourceKey::Id(id),
            KeyRef::Name { at, units } => {
                let bytes = image.window_bytes(
                    base + u64::from(at),
                    2 + u64::from(units) * 2,
                    "resources.name",
                )?;
                allocation.reserve(
                    "resource_name",
                    base + u64::from(at),
                    u64::from(units),
                    std::mem::size_of::<u16>() as u64,
                )?;
                let mut owned = Vec::with_capacity(units as usize);
                for pair in bytes[2..].as_chunks::<2>().0 {
                    owned.push(u16::from_le_bytes(*pair));
                }
                ResourceKey::Name(owned)
            }
        });
    }
    allocation.reserve(
        "resource_leaves",
        data.file_offset,
        1,
        std::mem::size_of::<ResourceLeaf>() as u64,
    )?;
    resources.leaves.push(ResourceLeaf { path, data: *data });

    // Only a three-level `RT_STRING` leaf with a numberable block id is a
    // string block; the format allows other depths and shapes, and those stay
    // plain leaves. The structure pass already checked both conditions and the
    // block's units, so `build` only books and copies.
    if let Some((block, language)) = string_leaf(keys) {
        let block = check_block_id(image, data, block)?;
        let built = build_string_block(image, block, language, data, allocation)?;
        allocation.reserve(
            "string_blocks",
            data.file_offset,
            1,
            std::mem::size_of::<StringBlock>() as u64,
        )?;
        resources.strings.push(built);
    }
    Ok(())
}

/// Decodes one `RT_STRING` block, booking every code unit and every decoded
/// text it hands out.
fn build_string_block(
    image: &Reader<'_>,
    block_id: u16,
    language: u32,
    data: &ResourceData,
    allocation: &mut AllocationBudget,
) -> Result<StringBlock, PeError> {
    let bytes = image.window_bytes(
        data.file_offset,
        u64::from(data.size),
        "resources.string_block",
    )?;
    let mut units = Vec::new();
    let mut at = 0usize;
    for index in 0..STRING_UNITS_PER_BLOCK {
        if at == bytes.len() {
            break;
        }
        let Some(length_bytes) = bytes.get(at..at + 2) else {
            return Err(PeError::string(
                image,
                data.file_offset + at as u64,
                "unit.length",
                "2 bytes of length".to_owned(),
                format!("{} bytes left", bytes.len() - at),
            ));
        };
        let length = usize::from(u16::from_le_bytes([length_bytes[0], length_bytes[1]]));
        let Some(raw) = bytes.get(at + 2..at + 2 + length * 2) else {
            return Err(PeError::string(
                image,
                data.file_offset + at as u64,
                "unit.code_units",
                format!("{} bytes of code units", length * 2),
                format!("{} bytes left", bytes.len() - at - 2),
            ));
        };
        allocation.reserve(
            "string_units",
            data.file_offset + at as u64,
            length as u64,
            std::mem::size_of::<u16>() as u64,
        )?;
        let mut code_units = Vec::with_capacity(length);
        for pair in raw.as_chunks::<2>().0 {
            code_units.push(u16::from_le_bytes(*pair));
        }
        allocation.reserve(
            "string_text",
            data.file_offset + at as u64,
            length as u64,
            1,
        )?;
        let text = String::from_utf16(&code_units).ok();
        units.push(StringUnit {
            index: index as u8,
            id: string_id(block_id, index as u8),
            code_units,
            text,
        });
        at += 2 + length * 2;
    }
    Ok(StringBlock {
        block_id,
        language,
        code_page: data.code_page,
        data: *data,
        units,
        trailing_bytes: (bytes.len() - at) as u32,
    })
}
