//! `accept_f12_b_*`: the bounded, cycle-checked PE resource reader
//! (`crate::pe_resources`), and `accept_f12_g_*`: the regression pins for the
//! F12-G measurement (`docs/findings/2026-09-29-f12-g-strings-dll-resources
//! -and-header-id-correlation.md`) — `strings.dll`'s two non-`RT_STRING`
//! leaves, their exact recorded bytes and the `RESOURCE.H`/`RESRC1.H` id
//! space's correlation with `langui.dll`'s string-table blocks.
//!
//! The fixtures are **newly authored** PE images: they follow the public
//! PE/COFF layout, and their ids, languages, code pages and string texts are
//! invented for this test. No original byte, string or resource name of the
//! installation is reproduced. The tests marked
//! `#[ignore = "requires CS_GAME_DIR"]` check the recorded *structural*
//! facts of the real images (counts, ids, code pages, sizes) instead.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::pe_resources::*;
use crate::text::dialect::{
    CRIMSON_ROF, MemberRule, TEXT_DIALECT_INVENTORY, TextDialect, dialect_for_member,
};
use crate::text::{ResourceHeader, read_resource_header};
use crate::{ParseContext, ParseErrorKind, RofLimits, read_member, read_tree};

/// The RVA the fixtures give their `.rsrc` section, so a payload's RVA is
/// this plus the offset it lands at inside the section.
const FIXTURE_RSRC_RVA: u32 = 0x2000;

// ------------------------------------------------------------ image builder

/// Assembles a minimal PE image, so a fixture describes *structure* — ids,
/// languages, code pages, sizes — instead of a wall of header constants.
struct PeBuilder {
    /// `e_lfanew`; `0x80` leaves room for the DOS stub.
    pe_offset: u32,
    /// `(name, virtual_address, virtual_size, raw_size)`. The raw pointer is
    /// assigned when the image is assembled.
    sections: Vec<(&'static str, u32, u32, u32)>,
    /// The bytes of the section holding the resource directory.
    rsrc: Vec<u8>,
    /// The resource data directory, when the image declares one.
    resource: Option<DataDirectory>,
    /// Overrides the section count after assembly, for a hostile image.
    section_count: Option<u16>,
}

impl PeBuilder {
    fn new() -> Self {
        Self {
            pe_offset: 0x80,
            sections: Vec::new(),
            rsrc: Vec::new(),
            resource: None,
            section_count: None,
        }
    }

    /// Declares a leading section, so the resource section is not the first
    /// one and the RVA translation crosses two mappings.
    fn with_leading_section(mut self, rva: u32, size: u32) -> Self {
        self.sections.push((".text", rva, size, size));
        self
    }

    /// Declares the `.rsrc` section at `rva` holding `bytes` and points the
    /// resource data directory at it.
    fn with_rsrc(mut self, rva: u32, bytes: Vec<u8>) -> Self {
        let size = u32::try_from(bytes.len()).expect("a fixture section fits a u32");
        self.sections.push((".rsrc", rva, size, size));
        self.rsrc = bytes;
        self.resource = Some(DataDirectory {
            virtual_address: rva,
            size,
        });
        self
    }

    /// Widens the declared resource-directory size without moving the
    /// section, so a fixture can claim more than it holds.
    fn with_resource_size(mut self, size: u32) -> Self {
        let rva = self.resource.expect("a resource directory").virtual_address;
        self.resource = Some(DataDirectory {
            virtual_address: rva,
            size,
        });
        self
    }

    /// Points the resource data directory at an RVA no section covers.
    fn with_resource_rva(mut self, rva: u32, size: u32) -> Self {
        self.resource = Some(DataDirectory {
            virtual_address: rva,
            size,
        });
        self
    }

    /// Sets a section's `VirtualSize` above its `SizeOfRawData`, so an RVA
    /// inside the section has no bytes behind it.
    fn with_uninitialised_tail(mut self, extra: u32) -> Self {
        let last = self.sections.len() - 1;
        self.sections[last].2 += extra;
        self
    }

    fn build(&self) -> Vec<u8> {
        const OPTIONAL_PE32: u16 = 224;
        let optional_size = OPTIONAL_PE32;
        let header_end = self.pe_offset as usize
            + 4
            + COFF_HEADER_BYTES as usize
            + optional_size as usize
            + self.sections.len() * SECTION_HEADER_BYTES as usize;
        let mut raw_pointer = (header_end + 0x1ff) & !0x1ff;
        let mut raw_pointers = Vec::new();
        for section in &self.sections {
            raw_pointers.push(raw_pointer as u32);
            raw_pointer += section.3 as usize;
        }

        let mut out = vec![0u8; header_end];
        out[0..2].copy_from_slice(&DOS_MAGIC);
        out[0x3c..0x40].copy_from_slice(&self.pe_offset.to_le_bytes());
        let signature = self.pe_offset as usize;
        out[signature..signature + 4].copy_from_slice(&PE_MAGIC);

        let coff = signature + 4;
        out[coff..coff + 2].copy_from_slice(&0x014cu16.to_le_bytes()); // i386
        let count = u16::try_from(self.sections.len()).expect("fixture sections");
        out[coff + 2..coff + 4].copy_from_slice(&self.section_count.unwrap_or(count).to_le_bytes());
        out[coff + 16..coff + 18].copy_from_slice(&optional_size.to_le_bytes());

        let optional = coff + COFF_HEADER_BYTES as usize;
        out[optional..optional + 2].copy_from_slice(&OPTIONAL_MAGIC_PE32.to_le_bytes());
        let directories = DATA_DIRECTORIES_OFFSET[0] as usize;
        let count_at = directories - 4;
        let directory_count: u32 = if self.resource.is_some() { 16 } else { 0 };
        out[optional + count_at..optional + count_at + 4]
            .copy_from_slice(&directory_count.to_le_bytes());
        out[optional + SIZE_OF_HEADERS_OFFSET as usize
            ..optional + SIZE_OF_HEADERS_OFFSET as usize + 4]
            .copy_from_slice(&(header_end as u32).to_le_bytes());
        if let Some(directory) = self.resource {
            let at = optional + directories + RESOURCE_DIRECTORY_INDEX as usize * 8;
            out[at..at + 4].copy_from_slice(&directory.virtual_address.to_le_bytes());
            out[at + 4..at + 8].copy_from_slice(&directory.size.to_le_bytes());
        }

        for (index, (name, virtual_address, virtual_size, raw_size)) in
            self.sections.iter().enumerate()
        {
            let row = optional + optional_size as usize + index * SECTION_HEADER_BYTES as usize;
            let name = name.as_bytes();
            out[row..row + name.len()].copy_from_slice(name);
            out[row + 8..row + 12].copy_from_slice(&virtual_size.to_le_bytes());
            out[row + 12..row + 16].copy_from_slice(&virtual_address.to_le_bytes());
            out[row + 16..row + 20].copy_from_slice(&raw_size.to_le_bytes());
            out[row + 20..row + 24].copy_from_slice(&raw_pointers[index].to_le_bytes());
        }

        if !self.rsrc.is_empty() {
            let start = *raw_pointers.last().expect("a resource section") as usize;
            out.resize(start + self.rsrc.len(), 0);
            out[start..start + self.rsrc.len()].copy_from_slice(&self.rsrc);
        }
        out
    }
}

/// A resource-directory writer, so a fixture says "type `RT_STRING`, block 1,
/// language 1033" instead of spelling out offsets.
#[derive(Default)]
struct Rsrc {
    bytes: Vec<u8>,
}

/// A directory inside [`Rsrc`], addressed by its offset relative to the
/// resource section's first byte.
type Dir = usize;

impl Rsrc {
    /// Reserves a directory with `count` id entries and returns its offset.
    fn dir(&mut self, count: usize) -> Dir {
        self.dir_with(0, count)
    }

    /// Reserves a directory with `named` name entries and `ids` id entries.
    /// The format requires the name entries to come first, which is the order
    /// the entries are written in.
    fn dir_with(&mut self, named: usize, ids: usize) -> Dir {
        let at = self.bytes.len();
        self.bytes
            .extend_from_slice(&[0u8; RESOURCE_DIRECTORY_HEADER_BYTES as usize]);
        self.bytes.extend_from_slice(&vec![
            0u8;
            (named + ids) * RESOURCE_DIRECTORY_ENTRY_BYTES as usize
        ]);
        self.bytes[at + 12..at + 14]
            .copy_from_slice(&u16::try_from(named).expect("fixture names").to_le_bytes());
        self.bytes[at + 14..at + 16]
            .copy_from_slice(&u16::try_from(ids).expect("fixture ids").to_le_bytes());
        at
    }

    /// Writes a numeric id as entry `index`'s name word.
    fn id(&mut self, directory: Dir, index: usize, id: u32) {
        let row = Self::row(directory, index);
        self.bytes[row..row + 4].copy_from_slice(&id.to_le_bytes());
    }

    /// Writes a UTF-16 name as the first named entry of `directory`,
    /// appending the name string to the section. The directory must have been
    /// reserved with [`Self::dir_with`] naming at least one named entry.
    fn name(&mut self, directory: Dir, name: &str) {
        let at = self.bytes.len();
        let units: Vec<u16> = name.encode_utf16().collect();
        self.bytes.extend_from_slice(
            &u16::try_from(units.len())
                .expect("fixture name")
                .to_le_bytes(),
        );
        for unit in units {
            self.bytes.extend_from_slice(&unit.to_le_bytes());
        }
        let row = Self::row(directory, 0);
        self.bytes[row..row + 4].copy_from_slice(&(HIGH_BIT | at as u32).to_le_bytes());
    }

    /// Points entry `index` at the subdirectory at `child`.
    fn sub(&mut self, directory: Dir, index: usize, child: Dir) {
        let row = Self::row(directory, index);
        self.bytes[row + 4..row + 8].copy_from_slice(&(HIGH_BIT | child as u32).to_le_bytes());
    }

    /// Points entry `index` at the data entry at `entry`.
    fn data_entry(&mut self, directory: Dir, index: usize, entry: usize) {
        let row = Self::row(directory, index);
        self.bytes[row + 4..row + 8].copy_from_slice(&(entry as u32).to_le_bytes());
    }

    /// Appends `payload` at the section's RVA plus where it lands and writes
    /// the data entry describing it.
    fn leaf_here(&mut self, payload: &[u8], code_page: u32) -> usize {
        let rva = FIXTURE_RSRC_RVA + self.bytes.len() as u32;
        self.leaf(rva, payload, code_page)
    }

    /// Appends `payload` and writes a data entry describing it, returning the
    /// data entry's offset. `rva` is the payload's RVA, which the fixture
    /// computes from where it lands in the section.
    fn leaf(&mut self, rva: u32, payload: &[u8], code_page: u32) -> usize {
        let size = u32::try_from(payload.len()).expect("fixture payload");
        self.bytes.extend_from_slice(payload);
        self.entry(rva, size, code_page)
    }

    /// Writes a data entry with an explicit extent, so a fixture can claim
    /// more bytes than the section holds.
    fn entry(&mut self, rva: u32, size: u32, code_page: u32) -> usize {
        let at = self.bytes.len();
        self.bytes.extend_from_slice(&rva.to_le_bytes());
        self.bytes.extend_from_slice(&size.to_le_bytes());
        self.bytes.extend_from_slice(&code_page.to_le_bytes());
        self.bytes.extend_from_slice(&0u32.to_le_bytes());
        at
    }

    /// A `RT_STRING` block: sixteen counted UTF-16LE units, the first
    /// `entries.len()` of them non-empty.
    fn string_payload(entries: &[&str]) -> Vec<u8> {
        let mut out = Vec::new();
        for index in 0..STRING_UNITS_PER_BLOCK {
            let text = entries.get(index).copied().unwrap_or("");
            let units: Vec<u16> = text.encode_utf16().collect();
            out.extend_from_slice(
                &u16::try_from(units.len())
                    .expect("fixture string")
                    .to_le_bytes(),
            );
            for unit in units {
                out.extend_from_slice(&unit.to_le_bytes());
            }
        }
        out
    }

    /// Creates the three-level path a `RT_STRING` leaf needs — type, name or
    /// id, language — and returns the language directory to hang leaves on.
    fn string_path(&mut self, type_id: u32, name: u32, language: u32) -> Dir {
        let types = self.dir(1);
        self.id(types, 0, type_id);
        let names = self.dir(1);
        self.id(names, 0, name);
        let languages = self.dir(1);
        self.id(languages, 0, language);
        self.sub(names, 0, languages);
        self.sub(types, 0, names);
        languages
    }

    fn row(directory: Dir, index: usize) -> usize {
        directory
            + RESOURCE_DIRECTORY_HEADER_BYTES as usize
            + index * RESOURCE_DIRECTORY_ENTRY_BYTES as usize
    }

    fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

/// The fixture image: a `.text` section, then a `.rsrc` section holding
/// `RT_STRING` with two blocks (one under en-US with two languages, one under
/// en-US alone) plus a second type reached through a *named* entry. Block
/// payloads are laid out after the directories, and each leaf's RVA is the
/// section RVA plus its payload's offset inside the section.
fn survey_shaped_image() -> Vec<u8> {
    const RSRC_RVA: u32 = FIXTURE_RSRC_RVA;
    let mut rsrc = Rsrc::default();

    let root = rsrc.dir(2);
    rsrc.id(root, 0, RT_STRING);
    rsrc.id(root, 1, 4001);

    // Level two: the name or id each block is selected by.
    let strings = rsrc.dir(3);
    rsrc.id(strings, 0, 1); // block 1
    rsrc.id(strings, 1, 2); // block 2
    rsrc.id(strings, 2, 3); // a third name, reached one level deeper
    rsrc.sub(root, 0, strings);

    // Block 1 under two languages: en-US, and a second id (`1`) that no
    // surveyed image uses, so the fixture pins that the same block id under
    // two languages is two strings, not one overwritten one.
    let block_1 = rsrc.dir(2);
    rsrc.id(block_1, 0, LANG_ENGLISH_US);
    rsrc.id(block_1, 1, LANG_ID_ONE);
    rsrc.sub(strings, 0, block_1);

    // Block 2 under en-US only.
    let block_2 = rsrc.dir(1);
    rsrc.id(block_2, 0, LANG_ENGLISH_US);
    rsrc.sub(strings, 1, block_2);

    // A second type whose second level is a *name*, then a language. Its
    // payload is six opaque bytes, not a string block, so the reader must not
    // interpret it as one.
    let names = rsrc.dir_with(1, 0);
    rsrc.name(names, "LOCALE");
    rsrc.sub(root, 1, names);
    let names_lang = rsrc.dir(1);
    rsrc.id(names_lang, 0, 0);
    rsrc.sub(names, 0, names_lang);

    // Now the leaves. A payload's RVA is the section's RVA plus the offset it
    // lands at, which the fixture knows because it appends the payload itself.
    let en_us = rsrc.leaf_here(&Rsrc::string_payload(&["alpha", "", "gamma"]), 1252);
    rsrc.data_entry(block_1, 0, en_us);
    let neutral = rsrc.leaf_here(&Rsrc::string_payload(&["alpha-neutral"]), 0);
    rsrc.data_entry(block_1, 1, neutral);
    let beta = rsrc.leaf_here(&Rsrc::string_payload(&["beta"]), 1252);
    rsrc.data_entry(block_2, 0, beta);

    // A `RT_STRING` leaf four levels deep. The format does not fix the
    // resource tree's depth, so the reader keeps the path as written and does
    // *not* read it as a string block: a block is exactly three deep.
    let deeper = rsrc.dir(1);
    rsrc.id(deeper, 0, 0);
    rsrc.sub(strings, 2, deeper);
    let deeper_lang = rsrc.dir(1);
    rsrc.id(deeper_lang, 0, LANG_ENGLISH_US);
    rsrc.sub(deeper, 0, deeper_lang);
    let four = rsrc.leaf_here(b"\x01\x00\x41\x00", 0);
    rsrc.data_entry(deeper_lang, 0, four);

    let opaque = rsrc.leaf_here(b"opaque", 1200);
    rsrc.data_entry(names_lang, 0, opaque);

    PeBuilder::new()
        .with_leading_section(0x1000, 0x400)
        .with_rsrc(RSRC_RVA, rsrc.finish())
        .build()
}

/// Reads `bytes` as the canonical fixture image.
fn resources(bytes: &[u8]) -> PeResources {
    read(bytes, "fixture.image").expect("an authored PE image reads")
}

/// Reads `bytes` as a PE image whose errors name `container`.
fn read(bytes: &[u8], container: &str) -> Result<PeResources, PeError> {
    let mut context = ParseContext::with_defaults(container);
    read_pe_resources(&mut context, bytes)
}

// ------------------------------------------------------- PE resource tests

/// The walk reaches every leaf and retains each key, the language id and the
/// data entry's code page: the three things the survey found and the three a
/// localized install has to keep stable while the display text changes.
#[test]
fn accept_f12_b_pe_resource_tree_keeps_ids_languages_and_code_pages() {
    let bytes = survey_shaped_image();
    let resources = resources(&bytes);

    // The header layout is read, not executed: sections and the resource
    // directory are located, nothing is mapped.
    let layout = resources.layout();
    assert_eq!(layout.optional_magic(), OPTIONAL_MAGIC_PE32);
    assert_eq!(layout.machine(), 0x014c);
    assert_eq!(layout.pe_offset(), 0x80);
    assert_eq!(layout.image_len(), bytes.len() as u64);
    assert_eq!(layout.sections().len(), 2);
    assert_eq!(layout.sections()[0].name, ".text");
    assert_eq!(layout.sections()[0].virtual_address, 0x1000);
    assert_eq!(layout.sections()[1].name, ".rsrc");
    assert_eq!(layout.sections()[1].virtual_address, FIXTURE_RSRC_RVA);
    let directory = layout.resource_directory().expect("a resource directory");
    assert_eq!(directory.virtual_address, FIXTURE_RSRC_RVA);
    assert!(u64::from(directory.size) <= u64::from(layout.sections()[1].raw_size));

    // Eight directories: the root, its two type subtrees, block 1's two
    // language subtrees, block 2's language subtree, the four-level name with
    // its language subtree, and the named type's language subtree. Five leaves,
    // of which three are three-deep `RT_STRING` blocks.
    assert_eq!(resources.directories(), 8);
    assert_eq!(resources.leaves().len(), 5);
    assert_eq!(resources.strings().len(), 3);

    // String ids follow `(block - 1) * 16 + index` and the text decodes.
    let alpha = resources
        .string_unit(string_id(1, 0), Some(LANG_ENGLISH_US))
        .expect("string 1:0 under en-US");
    assert_eq!(alpha.id, 0);
    assert_eq!(alpha.index, 0);
    assert_eq!(
        alpha.code_units,
        "alpha".encode_utf16().collect::<Vec<u16>>()
    );
    assert_eq!(alpha.text.as_deref(), Some("alpha"));
    // An empty unit is a unit: present, empty, and not a missing string.
    let empty = resources
        .string_unit(string_id(1, 1), Some(LANG_ENGLISH_US))
        .expect("string 1:1 is present but empty");
    assert!(empty.code_units.is_empty());
    assert_eq!(empty.text.as_deref(), Some(""));
    assert_eq!(
        resources
            .string_unit(string_id(1, 2), Some(LANG_ENGLISH_US))
            .map(|unit| unit.text.clone()),
        Some(Some("gamma".to_owned()))
    );
    // A second block's ids continue the same space.
    assert_eq!(
        resources
            .string_unit(string_id(2, 0), Some(LANG_ENGLISH_US))
            .map(|unit| unit.text.clone()),
        Some(Some("beta".to_owned()))
    );

    // The same id under a different language is a *different* string, not a
    // merged or overwritten one: a localized install keeps both.
    let neutral = resources
        .string_unit(string_id(1, 0), Some(LANG_ID_ONE))
        .expect("the neutral-language twin of string 1:0");
    assert_eq!(neutral.text.as_deref(), Some("alpha-neutral"));
    assert_ne!(
        resources.string_unit(string_id(1, 0), Some(LANG_ENGLISH_US)),
        resources.string_unit(string_id(1, 0), Some(LANG_ID_ONE))
    );
    assert_eq!(
        resources
            .string_unit(string_id(1, 0), None)
            .map(|unit| unit.text.clone()),
        Some(Some("alpha".to_owned()))
    );
    assert!(resources.string_unit(9000, None).is_none());

    // Every block keeps its own language id and code page verbatim, including
    // the compiler's "no code page" value `0`. Block 1 has one block *per
    // language*, so a block is identified by both.
    let mut blocks: Vec<(u16, u32, u32)> = resources
        .strings()
        .iter()
        .map(|block| (block.block_id, block.language, block.code_page))
        .collect();
    blocks.sort_unstable();
    assert_eq!(
        blocks,
        vec![
            (1, LANG_ID_ONE, 0),
            (1, LANG_ENGLISH_US, 1252),
            (2, LANG_ENGLISH_US, 1252),
        ]
    );
    let block_1_en = resources
        .string_block(1)
        .filter(|block| block.language == LANG_ENGLISH_US)
        .expect("block 1 under en-US");
    assert_eq!(block_1_en.units.len(), STRING_UNITS_PER_BLOCK);
    assert_eq!(block_1_en.trailing_bytes, 0);
    assert_eq!(block_1_en.data.code_page, 1252);
    assert!(resources.string_block(99).is_none());

    // Every leaf's language id is retained, and the two languages under block
    // 1 are two separate leaves.
    let mut languages: Vec<u32> = resources
        .leaves()
        .iter()
        .map(|leaf| {
            leaf.key(2)
                .and_then(ResourceKey::id)
                .expect("a language id")
        })
        .collect();
    languages.sort_unstable();
    assert_eq!(
        languages,
        vec![0, 0, LANG_ID_ONE, LANG_ENGLISH_US, LANG_ENGLISH_US]
    );
    // The four-deep leaf keeps its whole path and is not a string block.
    let deep = resources
        .leaves()
        .iter()
        .find(|leaf| leaf.path.len() == 4)
        .expect("the four-level leaf");
    assert_eq!(
        deep.path.iter().map(ResourceKey::id).collect::<Vec<_>>(),
        vec![Some(RT_STRING), Some(3), Some(0), Some(LANG_ENGLISH_US)]
    );

    // The named second level keeps its code units and reaches its language
    // subtree; the custom type id is retained as an id, not a name.
    let named = resources
        .leaves()
        .iter()
        .find(|leaf| leaf.key(1).is_some_and(ResourceKey::is_name))
        .expect("the named entry");
    let key = named.key(1).expect("a name key");
    assert_eq!(key.kind(), "name");
    assert_eq!(key.id(), None);
    assert_eq!(
        key.code_units(),
        Some("LOCALE".encode_utf16().collect::<Vec<u16>>().as_slice())
    );
    assert_eq!(key.text().as_deref(), Some("LOCALE"));
    assert_eq!(named.key(0).and_then(ResourceKey::id), Some(4001));
    assert_eq!(named.key(2).and_then(ResourceKey::id), Some(0));
    assert_eq!(named.path.len(), 3);
    assert_eq!(named.data.code_page, 1200);
    assert_eq!(named.data.size, 6);
    assert_eq!(named.data.reserved, 0);
    // Its payload is not a string block: six opaque bytes are not sixteen
    // counted UTF-16 units, and the reader leaves them alone.
    assert_eq!(resources.strings().len(), 3);

    // Every leaf's RVA is recorded verbatim *and* resolved to a file offset
    // inside a section's raw bytes, so the payload is really there.
    let rsrc_start = u64::from(layout.sections()[1].raw_pointer);
    let rsrc_size = u64::from(layout.sections()[1].raw_size);
    for leaf in resources.leaves() {
        // The data entry itself lives inside the resource directory, and the
        // payload it names inside the section's raw bytes.
        assert!(
            u64::from(leaf.data.directory_offset) + RESOURCE_DATA_ENTRY_BYTES
                <= u64::from(directory.size)
        );
        assert!(leaf.data.rva >= FIXTURE_RSRC_RVA);
        assert_eq!(
            leaf.data.file_offset,
            rsrc_start + u64::from(leaf.data.rva - FIXTURE_RSRC_RVA)
        );
        assert!(leaf.data.file_offset + u64::from(leaf.data.size) <= rsrc_start + rsrc_size);
        assert!(leaf.data.file_offset + u64::from(leaf.data.size) <= bytes.len() as u64);
    }
}

/// A hostile tree is refused, not followed: a cycle terminates, and every
/// offset is bounds-checked against the table or the image that holds it
/// (spec F12 non-negotiable #3).
#[test]
fn accept_f12_b_pe_resource_offsets_are_cycle_and_bounds_checked() {
    // A self-referential directory: the root's only entry points at the root.
    let mut rsrc = Rsrc::default();
    let root = rsrc.dir(1);
    rsrc.sub(root, 0, root);
    let bytes = PeBuilder::new().with_rsrc(0x2000, rsrc.finish()).build();
    let mut context = ParseContext::with_defaults("fixture.cycle");
    let error = read_pe_resources(&mut context, &bytes).expect_err("a cycle is refused");
    assert_eq!(error.code(), "directory_cycle");
    match error {
        PeError::DirectoryCycle { first, offset, .. } => {
            // Both are absolute file offsets: the resource directory starts
            // where its section's raw bytes do, which is not its RVA.
            let rsrc_start = u64::from(
                read_pe_layout(&mut ParseContext::with_defaults("probe"), &bytes)
                    .expect("layout")
                    .sections()[0]
                    .raw_pointer,
            );
            assert_eq!(first, rsrc_start, "the directory was first entered here");
            assert_eq!(offset, rsrc_start, "the entry that closed the cycle");
        }
        other => panic!("{other}"),
    }
    assert_eq!(
        context.allocation().used(),
        0,
        "a refused image books nothing beyond nothing"
    );

    // A two-directory cycle: root -> child -> root.
    let mut rsrc = Rsrc::default();
    let root = rsrc.dir(1);
    let child = rsrc.dir(1);
    rsrc.sub(root, 0, child);
    rsrc.sub(child, 0, root);
    let bytes = PeBuilder::new().with_rsrc(0x2000, rsrc.finish()).build();
    assert_eq!(
        read(&bytes, "fixture.cycle2")
            .expect_err("a two-level cycle")
            .code(),
        "directory_cycle"
    );

    // A subdirectory offset past the resource section.
    let mut rsrc = Rsrc::default();
    let root = rsrc.dir(1);
    rsrc.sub(root, 0, 0x0ff00);
    let bytes = PeBuilder::new().with_rsrc(0x2000, rsrc.finish()).build();
    let error = read(&bytes, "fixture.past").expect_err("an out-of-table offset is refused");
    assert_eq!(error.code(), "outside_table");
    assert_eq!(error.container(), "fixture.past");
    assert!(error.offset() >= 0x2000);

    // A directory table claiming more entries than the declared resource
    // directory holds: four entries need 48 bytes, the header declares 16.
    let mut rsrc = Rsrc::default();
    rsrc.dir(4);
    let bytes = PeBuilder::new()
        .with_rsrc(0x2000, rsrc.finish())
        .with_resource_size(RESOURCE_DIRECTORY_HEADER_BYTES as u32)
        .build();
    assert_eq!(
        read(&bytes, "fixture.table")
            .expect_err("a table past the section")
            .code(),
        "outside_table"
    );

    // A name whose code units reach past the section.
    let mut rsrc = Rsrc::default();
    let root = rsrc.dir(1);
    let at = rsrc.bytes.len();
    // A length of 4096 units with no units behind it.
    rsrc.bytes.extend_from_slice(&4096u16.to_le_bytes());
    let row = Rsrc::row(root, 0);
    rsrc.bytes[row..row + 4].copy_from_slice(&(HIGH_BIT | at as u32).to_le_bytes());
    let bytes = PeBuilder::new().with_rsrc(0x2000, rsrc.finish()).build();
    assert_eq!(
        read(&bytes, "fixture.name")
            .expect_err("an over-long name is refused")
            .code(),
        "outside_table"
    );

    // A data RVA inside a section's uninitialised tail: the section's
    // `VirtualSize` exceeds its raw bytes, so there is nothing to read even
    // though the RVA is inside the section.
    let mut rsrc = Rsrc::default();
    let leaf = rsrc.dir(1);
    let entry = rsrc.leaf(0x2ff0, b"x", 0);
    rsrc.data_entry(leaf, 0, entry);
    let bytes = PeBuilder::new()
        .with_rsrc(0x2000, rsrc.finish())
        .with_uninitialised_tail(0x1000)
        .build();
    let error = read(&bytes, "fixture.tail").expect_err("an RVA with no bytes is refused");
    assert_eq!(error.code(), "outside_table");

    // A data entry claiming more bytes than its section holds.
    let mut rsrc = Rsrc::default();
    let leaf = rsrc.dir(1);
    let entry = rsrc.entry(0x2000, 0x8000, 0);
    rsrc.bytes.extend_from_slice(b"short");
    rsrc.data_entry(leaf, 0, entry);
    let bytes = PeBuilder::new().with_rsrc(0x2000, rsrc.finish()).build();
    assert_eq!(
        read(&bytes, "fixture.size")
            .expect_err("an over-long data extent is refused")
            .code(),
        "outside_table"
    );

    // A data RVA no section covers at all.
    let mut rsrc = Rsrc::default();
    let leaf = rsrc.dir(1);
    let entry = rsrc.leaf(0x9000, b"x", 0);
    rsrc.data_entry(leaf, 0, entry);
    let bytes = PeBuilder::new().with_rsrc(0x2000, rsrc.finish()).build();
    assert_eq!(
        read(&bytes, "fixture.unmapped")
            .expect_err("an unmapped RVA is refused")
            .code(),
        "outside_table"
    );

    // A resource directory whose declared size exceeds its section.
    let mut rsrc = Rsrc::default();
    rsrc.dir(0);
    let bytes = PeBuilder::new()
        .with_rsrc(0x2000, rsrc.finish())
        .with_resource_size(0x4000)
        .build();
    assert_eq!(
        read(&bytes, "fixture.big")
            .expect_err("an over-large resource directory is refused")
            .code(),
        "outside_table"
    );

    // A resource directory smaller than one directory header.
    let mut rsrc = Rsrc::default();
    rsrc.dir(0);
    let bytes = PeBuilder::new()
        .with_rsrc(0x2000, rsrc.finish())
        .with_resource_size(4)
        .build();
    assert_eq!(
        read(&bytes, "fixture.tiny")
            .expect_err("a truncated resource directory is refused")
            .code(),
        "outside_table"
    );

    // A resource directory RVA that no section covers.
    let mut rsrc = Rsrc::default();
    rsrc.dir(0);
    let bytes = PeBuilder::new()
        .with_rsrc(0x2000, rsrc.finish())
        .with_resource_rva(0x9000, 0x40)
        .build();
    assert_eq!(
        read(&bytes, "fixture.rva")
            .expect_err("an unmapped directory RVA is refused")
            .code(),
        "outside_table"
    );
}

/// A `RT_STRING` block's counted units are bounds-checked against the data
/// entry's own extent: a unit that reaches past it is refused, a whole number
/// of units is kept, and trailing bytes are counted rather than dropped.
#[test]
fn accept_f12_b_string_blocks_are_bounds_checked() {
    // A unit claiming 40 code units in a block that holds 6 bytes.
    let mut rsrc = Rsrc::default();
    let leaf = rsrc.string_path(RT_STRING, 1, LANG_ENGLISH_US);
    let entry = rsrc.leaf_here(&[0x28, 0x00, 0x41, 0x00, 0x00, 0x00], 0);
    rsrc.data_entry(leaf, 0, entry);
    let bytes = PeBuilder::new().with_rsrc(0x2000, rsrc.finish()).build();
    let error = read(&bytes, "fixture.unit").expect_err("a unit past the block is refused");
    assert_eq!(error.code(), "string_block");

    // A block whose last unit's length word is cut in half.
    let mut rsrc = Rsrc::default();
    let leaf = rsrc.string_path(RT_STRING, 1, LANG_ENGLISH_US);
    let entry = rsrc.leaf_here(&[0x08, 0x00, 0x41], 0);
    rsrc.data_entry(leaf, 0, entry);
    let bytes = PeBuilder::new().with_rsrc(0x2000, rsrc.finish()).build();
    assert_eq!(
        read(&bytes, "fixture.half")
            .expect_err("a truncated length word is refused")
            .code(),
        "string_block"
    );

    // A whole block with two bytes left over: the trailing bytes are counted,
    // not skipped and not turned into a seventeenth unit.
    let mut payload = Rsrc::string_payload(&["only"]);
    payload.extend_from_slice(&[0, 0]);
    let mut rsrc = Rsrc::default();
    let leaf = rsrc.string_path(RT_STRING, 1, LANG_ENGLISH_US);
    let entry = rsrc.leaf_here(&payload, 1252);
    rsrc.data_entry(leaf, 0, entry);
    let bytes = PeBuilder::new().with_rsrc(0x2000, rsrc.finish()).build();
    let resources = resources(&bytes);
    let block = &resources.strings()[0];
    assert_eq!(block.units.len(), STRING_UNITS_PER_BLOCK);
    assert_eq!(block.trailing_bytes, 2);
    assert_eq!(block.units[0].text.as_deref(), Some("only"));
    assert_eq!(block.code_page, 1252);
    assert_eq!(block.data.size, payload.len() as u32);

    // A block id of 0 cannot number a string id, so it is refused rather than
    // silently renumbered into ids `0..16`.
    let mut rsrc = Rsrc::default();
    let langs = rsrc.string_path(RT_STRING, 0, 0);
    let entry = rsrc.leaf_here(&Rsrc::string_payload(&["x"]), 0);
    rsrc.data_entry(langs, 0, entry);
    let bytes = PeBuilder::new().with_rsrc(0x2000, rsrc.finish()).build();
    let error = read(&bytes, "fixture.block0").expect_err("a block id of 0 is refused");
    assert_eq!(error.code(), "string_block");
    match error {
        PeError::StringBlock { field, .. } => assert_eq!(field, "block.id"),
        other => panic!("{other}"),
    }

    // A `RT_STRING` block whose second-level id is wider than 16 bits.
    let mut rsrc = Rsrc::default();
    let langs = rsrc.string_path(RT_STRING, 0x0001_0000, 0);
    let entry = rsrc.leaf_here(&Rsrc::string_payload(&["x"]), 0);
    rsrc.data_entry(langs, 0, entry);
    let bytes = PeBuilder::new().with_rsrc(0x2000, rsrc.finish()).build();
    assert_eq!(
        read(&bytes, "fixture.wide")
            .expect_err("an over-wide block id is refused")
            .code(),
        "string_block"
    );

    // Unpaired surrogates are recorded, not replaced: the code units survive
    // and the text is absent rather than substituted.
    let payload = {
        let mut out = Vec::new();
        out.extend_from_slice(&1u16.to_le_bytes()); // one unit
        out.extend_from_slice(&0xd800u16.to_le_bytes()); // a lone high surrogate
        for _ in 1..STRING_UNITS_PER_BLOCK {
            out.extend_from_slice(&0u16.to_le_bytes());
        }
        out
    };
    let mut rsrc = Rsrc::default();
    let leaf = rsrc.string_path(RT_STRING, 1, LANG_ENGLISH_US);
    let entry = rsrc.leaf_here(&payload, 0);
    rsrc.data_entry(leaf, 0, entry);
    let bytes = PeBuilder::new().with_rsrc(0x2000, rsrc.finish()).build();
    let image = read(&bytes, "fixture.surrogate").expect("reads");
    let unit = &image.strings()[0].units[0];
    assert_eq!(unit.code_units, vec![0xd800]);
    assert_eq!(unit.text, None, "an unpaired surrogate has no text");
}

/// An image with no resource directory is a valid PE image rather than a
/// failure, and the layout reader refuses bytes that are not a PE image.
#[test]
fn accept_f12_b_pe_layout_and_directory_presence_are_checked() {
    // No resource directory: a well-formed image with one plain section.
    let mut builder = PeBuilder::new();
    builder.sections.push((".text", 0x1000, 16, 16));
    let bytes = builder.build();
    let resources = resources(&bytes);
    assert!(resources.layout().resource_directory().is_none());
    assert!(resources.leaves().is_empty());
    assert!(resources.strings().is_empty());
    assert_eq!(resources.directories(), 0);
    assert_eq!(resources.string_unit(1, None), None);

    // Not an image at all.
    assert_eq!(
        read(b"not a pe image at all", "fixture.mz")
            .expect_err("no MZ")
            .code(),
        "malformed"
    );
    // A DOS header with no PE signature.
    let mut bytes = survey_shaped_image();
    bytes[0x80] = b'X';
    let error = read(&bytes, "fixture.pe").expect_err("no PE signature");
    assert_eq!(error.code(), "malformed");
    assert_eq!(error.offset(), 0x80);
    // An optional-header shape this reader does not know.
    let mut bytes = survey_shaped_image();
    let optional = 0x80 + 4 + COFF_HEADER_BYTES as usize;
    bytes[optional..optional + 2].copy_from_slice(&0x9999u16.to_le_bytes());
    assert_eq!(
        read(&bytes, "fixture.magic")
            .expect_err("an unknown optional-header magic")
            .code(),
        "malformed"
    );
    // A truncated image: the last section's bytes are cut off.
    let bytes = survey_shaped_image();
    let cut = &bytes[..bytes.len() - 0x200];
    let error = read(cut, "fixture.cut").expect_err("a truncated image");
    assert_eq!(error.container(), "fixture.cut");
    // How a truncation is reported depends on which structure it cuts: the
    // section table, a section's raw pointer, or a data extent. All three are
    // refusals with a located diagnostic, and none is a panic.
    assert!(
        matches!(error.code(), "parse" | "outside_table"),
        "a truncated image is refused: {error}"
    );

    // `read_pe_layout` on its own is the same reader, one stage short.
    let bytes = survey_shaped_image();
    let mut context = ParseContext::with_defaults("fixture.layout");
    let layout = read_pe_layout(&mut context, &bytes).expect("reads");
    assert_eq!(layout.sections().len(), 2);
    assert_eq!(layout.optional_magic(), OPTIONAL_MAGIC_PE32);
    // An RVA maps into a section, into the headers, and nowhere else.
    let rsrc = &layout.sections()[1];
    let inside = layout
        .map_rva(rsrc.virtual_address + 4)
        .expect("inside .rsrc");
    assert_eq!(inside.file_offset, u64::from(rsrc.raw_pointer) + 4);
    assert_eq!(inside.available, u64::from(rsrc.raw_size) - 4);
    let headers = layout.map_rva(0).expect("the headers are mapped");
    assert_eq!(headers.file_offset, 0);
    assert!(headers.available <= u64::from(layout.headers_size()));
    assert!(layout.map_rva(0x7000).is_none(), "no section covers 0x7000");
}

/// The reader's own budgets: a resource section that fits a small budget
/// reads, one byte less is refused with the F03 error kind rather than a
/// panic, and the refusals leave the ledger consistent.
#[test]
fn accept_f12_b_pe_resources_are_bounded_by_the_allocation_budget() {
    let bytes = survey_shaped_image();
    let mut probe = ParseContext::with_defaults("fixture.probe");
    read_pe_resources(&mut probe, &bytes).expect("reads");
    let needed = probe.allocation().used();
    assert!(needed > 0, "the reader books what it allocates");

    let mut exact = ParseContext::new("fixture.exact", needed, 8);
    read_pe_resources(&mut exact, &bytes).expect("exactly the budget it books is enough");
    assert_eq!(exact.allocation().used(), needed);

    let mut short = ParseContext::new("fixture.short", needed - 1, 8);
    let error = read_pe_resources(&mut short, &bytes).expect_err("one byte short is refused");
    match &error {
        PeError::Parse(error) => {
            assert_eq!(error.kind, ParseErrorKind::AllocationBudgetExceeded);
            assert_eq!(error.container, "fixture.short");
            assert!(error.field.starts_with(PE_RESOURCES_ENTRYPOINT));
        }
        other => panic!("{other}"),
    }
    // The structure pass had already succeeded before the build pass was
    // refused, so it keeps its own charges and the build's are rolled back.
    assert!(
        short.allocation().used() < needed,
        "a rolled-back attempt keeps only what it read"
    );

    // A budget of 0 refuses the section table in the layout pass, and books
    // nothing.
    let mut none = ParseContext::new("fixture.none", 0, 8);
    let error = read_pe_resources(&mut none, &bytes).expect_err("no budget at all is refused");
    assert_eq!(error.code(), "parse");
    assert_eq!(none.allocation().used(), 0);

    // A recursion budget of 0 refuses the first directory, in the structure
    // pass, before anything is built.
    let mut flat = ParseContext::new("fixture.flat", needed, 0);
    let error = read_pe_resources(&mut flat, &bytes).expect_err("no nesting is allowed");
    match error {
        PeError::Parse(error) => {
            assert_eq!(error.kind, ParseErrorKind::RecursionDepthExceeded);
            assert!(error.field.contains("resources.directory"));
        }
        other => panic!("{other}"),
    }

    // A recursion budget deep enough for the fixture but not for one more
    // level refuses the deeper tree, and the same bytes read with a budget
    // that fits.
    let mut shallow = ParseContext::new("fixture.shallow", needed, 2);
    let error = read_pe_resources(&mut shallow, &bytes).expect_err("two levels is not enough");
    assert!(matches!(
        error,
        PeError::Parse(ref error) if error.kind == ParseErrorKind::RecursionDepthExceeded
    ));
    let mut deep = ParseContext::new("fixture.deep", needed, 6);
    read_pe_resources(&mut deep, &bytes).expect("six levels is enough");
}

// ------------------------------------------------------------ retail checks

/// The recorded structural facts of the three surveyed PE images: PE32,
/// `machine 0x014c`, a `.rsrc` section, exactly one resource type, the
/// recorded number of `RT_STRING` blocks, the recorded language ids and the
/// recorded code pages. It compares structure only — no string text of the
/// installation is compared, printed or kept.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f12_b_retail_pe_resource_structure_matches_the_survey() {
    struct Surveyed {
        path: &'static str,
        size: u64,
        /// Every resource type the image carries, as `(type id, leaf count)`.
        types: &'static [(u32, usize)],
        /// `RT_STRING` blocks, which is what `strings()` decodes.
        blocks: usize,
        /// Every leaf's language id, deduplicated.
        languages: &'static [u32],
        /// Every data entry's code page, deduplicated.
        code_pages: &'static [u32],
        /// Directories and leaves the whole resource tree holds.
        directories: u32,
        leaves: usize,
    }

    const SURVEY: &[Surveyed] = &[
        Surveyed {
            path: "strings.dll",
            size: 131_072,
            // The survey found three types: `RT_STRING`, `RT_VERSION`, and an
            // unassigned id 255. The reader records all of them.
            types: &[(RT_STRING, 112), (16, 1), (255, 1)],
            blocks: 112,
            languages: &[LANG_ENGLISH_US],
            code_pages: &[1252],
            directories: 118,
            leaves: 114,
        },
        Surveyed {
            path: "GOSDATA/ASSETS/BINARIES/language.dll",
            size: 32_768,
            types: &[(RT_STRING, 3)],
            blocks: 3,
            languages: &[LANG_ENGLISH_US],
            code_pages: &[0],
            directories: 5,
            leaves: 3,
        },
        Surveyed {
            path: "GOSDATA/ASSETS/BINARIES/langui.dll",
            size: 282_624,
            types: &[(RT_STRING, 101)],
            blocks: 101,
            languages: &[LANG_ENGLISH_US],
            code_pages: &[0],
            directories: 103,
            leaves: 101,
        },
    ];

    let dir = std::env::var_os("CS_GAME_DIR")
        .expect("CS_GAME_DIR is not set: this test needs the original installation");
    let dir = std::path::PathBuf::from(dir);

    for expected in SURVEY {
        // The inventory routes exactly these three loose files to the
        // `pe.resources` dialect, and nothing else.
        let rule = TEXT_DIALECT_INVENTORY
            .iter()
            .flat_map(|record| record.members.iter())
            .find(|rule| rule.matches(expected.path, None))
            .unwrap_or_else(|| panic!("the inventory does not route {}", expected.path));
        let MemberRule::Loose { path, length } = *rule else {
            panic!("{} is routed as a member", expected.path)
        };
        assert_eq!(path, expected.path);
        assert_eq!(length, expected.size);
        assert_eq!(
            dialect_for_member(expected.path, None),
            Some(TextDialect::PeResources)
        );

        let file = dir.join(expected.path);
        let bytes = std::fs::read(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        assert_eq!(bytes.len() as u64, expected.size);

        let mut context = ParseContext::with_defaults(expected.path);
        let resources = read_pe_resources(&mut context, &bytes)
            .unwrap_or_else(|error| panic!("{}: {error}", expected.path));
        let layout = resources.layout();
        assert_eq!(
            layout.optional_magic(),
            OPTIONAL_MAGIC_PE32,
            "{}",
            expected.path
        );
        assert_eq!(layout.machine(), 0x014c, "{}", expected.path);
        assert!(layout.resource_directory().is_some(), "{}", expected.path);
        assert!(
            layout
                .sections()
                .iter()
                .any(|section| section.name == ".rsrc"),
            "{}: a .rsrc section",
            expected.path
        );

        // The whole tree is walked: the recorded directory and leaf counts.
        assert_eq!(
            resources.directories(),
            expected.directories,
            "{}",
            expected.path
        );
        assert_eq!(
            resources.leaves().len(),
            expected.leaves,
            "{}",
            expected.path
        );
        assert_eq!(
            resources.strings().len(),
            expected.blocks,
            "{}",
            expected.path
        );

        // Every resource type the survey found is present, with the number of
        // leaves it holds. `strings.dll` carries two types besides `RT_STRING`
        // and this records them rather than reading only the strings.
        let mut types: Vec<(u32, usize)> = Vec::new();
        for leaf in resources.leaves() {
            let id = leaf.key(0).and_then(ResourceKey::id).expect("a type id");
            match types.iter_mut().find(|(seen, _)| *seen == id) {
                Some((_, count)) => *count += 1,
                None => types.push((id, 1)),
            }
        }
        types.sort_unstable();
        let mut expected_types: Vec<(u32, usize)> = expected.types.to_vec();
        expected_types.sort_unstable();
        assert_eq!(types, expected_types, "{}", expected.path);

        // Every leaf's language id and code page, deduplicated, match the
        // survey: `strings.dll` under en-US with code page 1252, the other two
        // under en-US with the resource compiler's "no code page" value 0.
        let mut leaf_languages: Vec<u32> = resources
            .leaves()
            .iter()
            .filter_map(|leaf| leaf.key(2).and_then(ResourceKey::id))
            .collect();
        leaf_languages.sort_unstable();
        leaf_languages.dedup();
        assert_eq!(leaf_languages, expected.languages, "{}", expected.path);
        let mut code_pages: Vec<u32> = resources
            .leaves()
            .iter()
            .map(|leaf| leaf.data.code_page)
            .collect();
        code_pages.sort_unstable();
        code_pages.dedup();
        assert_eq!(code_pages, expected.code_pages, "{}", expected.path);

        // Block ids are unique, and every unit is a whole counted unit whose
        // id follows the documented `(block - 1) * 16 + index` rule.
        let mut block_ids: Vec<u16> = resources.strings().iter().map(|b| b.block_id).collect();
        block_ids.sort_unstable();
        let unique = block_ids.len();
        block_ids.dedup();
        assert_eq!(block_ids.len(), unique, "{}", expected.path);
        for block in resources.strings() {
            assert!(
                block.units.len() <= STRING_UNITS_PER_BLOCK,
                "{}: block {} has {} units",
                expected.path,
                block.block_id,
                block.units.len()
            );
            assert_eq!(
                block.trailing_bytes, 0,
                "{}: block {}",
                expected.path, block.block_id
            );
            for unit in &block.units {
                assert_eq!(unit.id, string_id(block.block_id, unit.index));
                // A retail English string decodes; a unit that did not would
                // mean the code units were not text, which is recorded as an
                // absent text rather than a replaced one.
                assert!(
                    unit.text.is_some(),
                    "{}: block {} unit {} did not decode",
                    expected.path,
                    block.block_id,
                    unit.index
                );
            }
        }
    }
}

// ----------------------------------------------------------- accept_f12_g_*
//
// Regression pins for the F12-G measurement (Rally #368), written by F12-K
// (#377). The recorded facts —
// `docs/findings/2026-09-29-f12-g-strings-dll-resources-and-header-id-correlation.md` —
// were measured on the installation whose `install_sha256` is
// `b4e780ab…`: `strings.dll` carries three resource types, two of them not
// `RT_STRING` (a 944-byte type-16 `VS_VERSIONINFO` and a 4-byte type-255 leaf
// whose meaning stays unknown), and 775 of the 782 distinct resource ids the
// two `.H` members declare name a block `langui.dll` actually has under the
// `(block - 1) * 16 + index` numbering. The retail tests below re-derive every
// pinned number from the production readers (`read_pe_resources`,
// `read_tree`/`read_member`, `read_resource_header`); the two unignored tests
// pin the reader's handling of non-`RT_STRING` leaves and the invariant that
// no production path interprets the type-255 payload.

/// `strings.dll`, whole image: 131 072 bytes.
const F12_G_STRINGS_DLL_SHA256: &str =
    "7582fecaca42d21dd44eb95f896dcb415af790c7057ad81c8e1600ac0b445c21";

/// The type-16 leaf's 944-byte `VS_VERSIONINFO` payload.
const F12_G_TYPE_16_SHA256: &str =
    "2d9ed5039fedf0cacaa7a9b84732921863ad7e6b7dfec339c8236c4ac5f705cd";

/// The type-255 leaf's 4-byte payload.
const F12_G_TYPE_255_SHA256: &str =
    "641c2b20cfae89ad63861b5b6a0142bd371f17d9a4002e2983baa7aca9f062a6";

/// `ASSETS/SCRIPTS/RESOURCE.H`, decoded: 29 579 bytes.
const F12_G_RESOURCE_H_SHA256: &str =
    "61ec23270fdf1dc484085db93c936af4e4bb5bb177e3d8a3dd513a6bc4eefb78";

/// `ASSETS/SCRIPTS/RESRC1.H`, decoded: 8 922 bytes.
const F12_G_RESRC1_H_SHA256: &str =
    "5d9c896d7532a022a40c1e733eb5d21b7ca85219be4633e23ae69d88cb649c52";

/// The measured `VS_VERSIONINFO` node table of `strings.dll`'s type-16 leaf:
/// `(depth, offset inside the payload, wLength, wValueLength, wType, key)`,
/// preorder. The keys are the documented version-resource names — structural
/// field names, not original content — and the table tiles the payload with
/// no trailing bytes.
const F12_G_VERSION_INFO_NODES: &[(usize, usize, u16, u16, u16, &str)] = &[
    (0, 0x000, 944, 52, 0, "VS_VERSION_INFO"),
    (1, 0x05c, 784, 0, 1, "StringFileInfo"),
    (2, 0x080, 748, 0, 1, "040904b0"),
    (3, 0x098, 26, 1, 1, "Comments"),
    (3, 0x0b4, 76, 22, 1, "CompanyName"),
    (3, 0x100, 94, 27, 1, "FileDescription"),
    (3, 0x160, 40, 4, 1, "FileVersion"),
    (3, 0x188, 48, 8, 1, "InternalName"),
    (3, 0x1b8, 114, 39, 1, "LegalCopyright"),
    (3, 0x22c, 42, 1, 1, "LegalTrademarks"),
    (3, 0x258, 64, 12, 1, "OriginalFilename"),
    (3, 0x298, 34, 1, 1, "PrivateBuild"),
    (3, 0x2bc, 80, 24, 1, "ProductName"),
    (3, 0x30c, 58, 11, 1, "ProductVersion"),
    (3, 0x348, 34, 1, 1, "SpecialBuild"),
    (1, 0x36c, 68, 0, 1, "VarFileInfo"),
    (2, 0x38c, 36, 4, 0, "Translation"),
];

/// The PE images the F12-G survey scored the two headers' 782 distinct values
/// against: `(install-relative spelling, RT_STRING block count, values naming
/// a block under the `(block - 1) * 16 + index` numbering)`.
const F12_G_IMAGE_SCORES: &[(&str, usize, usize)] = &[
    ("GOSDATA/ASSETS/BINARIES/langui.dll", 101, 775),
    ("SETUPENU.DLL", 37, 169),
    ("strings.dll", 112, 123),
    ("ebueula.dll", 8, 28),
    ("crimson.icd", 2, 15),
    ("clokspl.exe", 19, 14),
    ("UNINSTAL.EXE", 17, 12),
    ("dsetup32.dll", 7, 10),
    ("GOSDATA/ASSETS/BINARIES/language.dll", 3, 2),
    ("mcp.dll", 5, 0),
    ("mfc42.dll", 43, 0),
];

/// The seven header values that name no `langui.dll` block under either
/// numbering (recorded as unexplained, not guessed at).
const F12_G_HEADER_MISSES: [u32; 7] = [600, 620, 2002, 2050, 2054, 3510, 3540];

/// The eighteen `langui.dll` blocks no header value addresses.
const F12_G_UNADDRESSED_BLOCKS: [u16; 18] = [
    2, 3, 4, 5, 190, 195, 196, 197, 198, 200, 201, 202, 204, 205, 206, 217, 219, 227,
];

/// The empty units of blocks 2501–2511 that `RESRC1.H` *does* name — the six
/// ids proving the tail agreement is the five omitted ids alone.
const F12_G_NAMED_EMPTY_UNITS: [u32; 6] = [40001, 40014, 40036, 40040, 40054, 40080];

/// The root of the read-only original installation, or a loud failure: a
/// retail test must fail, not pass, when `CS_GAME_DIR` is absent.
fn f12_g_game_dir() -> PathBuf {
    let dir = std::env::var_os("CS_GAME_DIR").expect(
        "CS_GAME_DIR is not set: this test needs the original installation \
         (capability `retail`)",
    );
    let dir = PathBuf::from(dir);
    assert!(
        dir.is_dir(),
        "CS_GAME_DIR {} is not a directory",
        dir.display()
    );
    dir
}

/// The bytes of the installation file `spelling` names (a `/`-separated path
/// relative to the installation root).
fn f12_g_file(dir: &Path, spelling: &str) -> Vec<u8> {
    let mut path = dir.to_path_buf();
    for segment in spelling.split('/') {
        path.push(segment);
    }
    std::fs::read(&path)
        .unwrap_or_else(|error| panic!("{spelling}: the installation must hold it: {error}"))
}

/// `image`'s resource tree, read through the production PE resource reader.
fn f12_g_resources(image: &[u8], name: &str) -> PeResources {
    let mut context = ParseContext::with_defaults(name);
    read_pe_resources(&mut context, image)
        .unwrap_or_else(|error| panic!("{name}: the production reader must read it: {error}"))
}

/// The decoded bytes of `member` (a `/`-separated spelling) inside `rof`,
/// reached through the production `read_tree` walk and `read_member` decode —
/// never by scanning the container's bytes by hand.
fn f12_g_rof_member(rof: &[u8], member: &str) -> Vec<u8> {
    let mut context = ParseContext::with_defaults(CRIMSON_ROF);
    let tree = read_tree(&mut context, rof).expect("the container's tree must walk");
    let wanted: Vec<&str> = member.split('/').collect();
    let found = tree
        .members()
        .iter()
        .find(|entry| {
            entry.path.len() == wanted.len()
                && entry
                    .path
                    .iter()
                    .zip(&wanted)
                    .all(|(segment, name)| segment.eq_ignore_ascii_case(name.as_bytes()))
        })
        .unwrap_or_else(|| panic!("{member}: no such member in {CRIMSON_ROF}"));
    let read = read_member(&context, rof, found, &RofLimits::default())
        .unwrap_or_else(|error| panic!("{member}: the member must read: {error}"));
    assert_eq!(
        read.trailing_len, 0,
        "{member}: no bytes sit unread inside the stored extent"
    );
    read.data
}

/// The leaf at the numeric path `ids` (`[type, name, language]`), or a loud
/// failure naming what was asked for.
fn f12_g_leaf(resources: &PeResources, ids: [u32; 3]) -> &ResourceLeaf {
    resources
        .leaves()
        .iter()
        .find(|leaf| {
            leaf.path.len() == 3
                && leaf
                    .path
                    .iter()
                    .zip(ids)
                    .all(|(key, id)| key.id() == Some(id))
        })
        .unwrap_or_else(|| panic!("no leaf at {ids:?}"))
}

/// Whether `leaf` is a three-level `RT_STRING` leaf — the shape the reader's
/// `string_leaf` decodes into a `StringBlock`. The retail tests pin that the
/// leaves failing this shape are exactly the two measured non-string leaves,
/// which is what keeps this predicate honest rather than a private copy.
fn f12_g_string_leaf(leaf: &ResourceLeaf) -> bool {
    leaf.path.len() == 3
        && leaf.key(0) == Some(&ResourceKey::Id(RT_STRING))
        && leaf.key(1).is_some_and(|key| key.id().is_some())
        && leaf.key(2).is_some_and(|key| key.id().is_some())
}

/// The set of block ids `strings()` reports — the `RT_STRING` blocks the
/// image has, which is what a header id can name.
fn f12_g_block_ids(resources: &PeResources) -> BTreeSet<u16> {
    resources
        .strings()
        .iter()
        .map(|block| block.block_id)
        .collect()
}

/// Whether `name` is one of the observed string-table prefixes
/// (`IDS_` / `SB_` / `STR_`) — the names the finding counts a define under.
fn f12_g_string_name(name: &[u8]) -> bool {
    name.starts_with(b"IDS_") || name.starts_with(b"SB_") || name.starts_with(b"STR_")
}

/// The distinct resource ids `header`'s defines name.
fn f12_g_define_ids(header: &ResourceHeader<'_>, string_names_only: bool) -> BTreeSet<u32> {
    header
        .defines()
        .filter(|define| !string_names_only || f12_g_string_name(define.name))
        .filter_map(|define| define.resource_id())
        .collect()
}

/// One node of the recorded `VS_VERSIONINFO` walk: where it sits inside the
/// payload and the four fields of its header. The node names are the
/// documented version-resource keys — structure, not content.
#[derive(Debug, PartialEq, Eq)]
struct F12GVersionNode {
    depth: usize,
    offset: usize,
    length: u16,
    value_length: u16,
    kind: u16,
    key: String,
}

/// Walks `payload` as the recorded `VS_VERSIONINFO` tree: each node is
/// `wLength`, `wValueLength`, `wType`, a NUL-terminated UTF-16 key, a
/// 4-aligned value (`wValueLength` units for `wType` 1, bytes for 0) and
/// children tiling the rest of its extent. Returns the nodes in preorder and
/// the number of bytes the root consumed, so the caller can assert the tree
/// explains the payload *exactly* — no trailing bytes.
///
/// This is the measurement check, not a reader: production has no version-info
/// parser and these tests exist precisely so none is needed to keep the
/// recorded structure pinned.
fn f12_g_version_nodes(payload: &[u8]) -> (Vec<F12GVersionNode>, usize) {
    fn u16_at(payload: &[u8], at: usize) -> u16 {
        u16::from_le_bytes([payload[at], payload[at + 1]])
    }
    fn walk(payload: &[u8], at: usize, depth: usize, nodes: &mut Vec<F12GVersionNode>) -> usize {
        let length = u16_at(payload, at);
        let value_length = u16_at(payload, at + 2);
        let kind = u16_at(payload, at + 4);
        let mut cursor = at + 6;
        let mut key = Vec::new();
        loop {
            let unit = u16_at(payload, cursor);
            cursor += 2;
            if unit == 0 {
                break;
            }
            key.push(unit);
        }
        nodes.push(F12GVersionNode {
            depth,
            offset: at,
            length,
            value_length,
            kind,
            key: String::from_utf16(&key).expect("the recorded keys are UTF-16"),
        });
        let value_bytes = if kind == 1 {
            usize::from(value_length) * 2
        } else {
            usize::from(value_length)
        };
        let end = at + usize::from(length);
        let mut child = ((cursor + 3) & !3) + value_bytes;
        child = (child + 3) & !3;
        while child < end {
            child = walk(payload, child, depth + 1, nodes);
        }
        (end + 3) & !3
    }
    let mut nodes = Vec::new();
    let consumed = walk(payload, 0, 0, &mut nodes);
    (nodes, consumed)
}

/// `strings.dll`'s resource tree carries exactly the three measured types —
/// `RT_STRING` plus the two non-string leaves — and the reader's
/// "other leaves" count (the string catalog's `other_leaves`) is exactly
/// those two, named here by their paths.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f12_g_retail_strings_dll_tree_and_its_two_other_leaves() {
    let dir = f12_g_game_dir();
    let image = f12_g_file(&dir, "strings.dll");
    assert_eq!(image.len(), 131_072, "strings.dll is the recorded image");
    assert_eq!(
        cs_assets::install::sha256(&image).to_hex(),
        F12_G_STRINGS_DLL_SHA256
    );

    let resources = f12_g_resources(&image, "strings.dll");
    let directory = resources
        .layout()
        .resource_directory()
        .expect("the image declares a resource directory");
    assert_eq!(directory.virtual_address, 0x1_2000);
    assert_eq!(directory.size, 46_712);
    assert_eq!(resources.directories(), 118);
    assert_eq!(resources.leaves().len(), 114);
    assert_eq!(resources.strings().len(), 112);

    // Exactly three resource types, with the recorded leaf counts and payload
    // byte totals.
    let mut types: std::collections::BTreeMap<u32, (usize, u64)> =
        std::collections::BTreeMap::new();
    for leaf in resources.leaves() {
        let type_id = leaf
            .key(0)
            .and_then(ResourceKey::id)
            .expect("every surveyed leaf has a numeric type");
        let entry = types.entry(type_id).or_default();
        entry.0 += 1;
        entry.1 += u64::from(leaf.data.size);
    }
    assert_eq!(
        types.into_iter().collect::<Vec<_>>(),
        vec![(RT_STRING, (112, 40_086)), (16, (1, 944)), (255, (1, 4))],
    );

    // The two "other leaves" are exactly the type-16 and type-255 leaves at
    // name id 1, language en-US — and the reader's accounting holds nothing
    // else: 114 leaves = 112 string blocks + these two.
    let other: Vec<Vec<u32>> = resources
        .leaves()
        .iter()
        .filter(|leaf| !f12_g_string_leaf(leaf))
        .map(|leaf| {
            leaf.path
                .iter()
                .map(|key| key.id().expect("a numeric leaf path"))
                .collect()
        })
        .collect();
    assert_eq!(other, vec![vec![16, 1, 1033], vec![255, 1, 1033]]);
    assert_eq!(
        resources.leaves().len() - resources.strings().len(),
        other.len(),
        "every non-string leaf is a retained leaf, and vice versa"
    );
}

/// The type-16 leaf is the recorded `VS_VERSIONINFO`: its data entry sits at
/// the measured offsets, its payload digests to the measured hash and the
/// whole 944 bytes walk as the 17 recorded nodes.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f12_g_retail_type_16_leaf_is_a_complete_vs_versioninfo() {
    let dir = f12_g_game_dir();
    let image = f12_g_file(&dir, "strings.dll");
    let resources = f12_g_resources(&image, "strings.dll");

    let leaf = f12_g_leaf(&resources, [16, 1, 1033]);
    assert_eq!(leaf.key(0), Some(&ResourceKey::Id(16)));
    assert_eq!(leaf.key(1), Some(&ResourceKey::Id(1)));
    assert_eq!(leaf.key(2), Some(&ResourceKey::Id(LANG_ENGLISH_US)));
    // The data entry is the 39th (0-indexed) one in the directory walk, at
    // 0x12000 + 0x1598 in the file; the reader keeps that offset.
    assert_eq!(leaf.data.directory_offset, 0x1598);
    assert_eq!(leaf.data.rva, 0x1_d2c4);
    assert_eq!(leaf.data.file_offset, 0x1_d2c4);
    assert_eq!(leaf.data.size, 944);
    assert_eq!(leaf.data.code_page, 1252);
    assert_eq!(leaf.data.reserved, 0);

    let payload = &image
        [leaf.data.file_offset as usize..leaf.data.file_offset as usize + leaf.data.size as usize];
    assert_eq!(
        cs_assets::install::sha256(payload).to_hex(),
        F12_G_TYPE_16_SHA256
    );

    // The recorded walk: the root carries a VS_FIXEDFILEINFO (52 bytes,
    // signature 0xFEEF04BD at payload offset 40) and the whole tree tiles the
    // payload — 17 nodes, at most three levels deep, nothing trailing.
    let (nodes, consumed) = f12_g_version_nodes(payload);
    assert_eq!(nodes.len(), F12_G_VERSION_INFO_NODES.len());
    assert_eq!(consumed, payload.len(), "no trailing bytes after the root");
    let root = &nodes[0];
    assert_eq!(root.key, "VS_VERSION_INFO");
    assert_eq!(usize::from(root.length), payload.len());
    assert_eq!(root.value_length, 52);
    assert_eq!(root.kind, 0);
    assert_eq!(
        u32::from_le_bytes([payload[40], payload[41], payload[42], payload[43]]),
        0xFEEF_04BD,
        "the VS_FIXEDFILEINFO signature sits at the recorded offset"
    );
    for (node, (depth, offset, length, value_length, kind, key)) in
        nodes.iter().zip(F12_G_VERSION_INFO_NODES)
    {
        assert_eq!(node.depth, *depth, "node at {offset:#x}");
        assert_eq!(node.offset, *offset);
        assert_eq!(node.length, *length, "node {key}");
        assert_eq!(node.value_length, *value_length, "node {key}");
        assert_eq!(node.kind, *kind, "node {key}");
        assert_eq!(node.key, *key);
    }
    assert!(
        nodes.iter().all(|node| node.depth <= 3),
        "the walk never goes deeper than the recorded three levels"
    );
}

/// The type-255 leaf is four bytes at the measured offset with the measured
/// payload. Its meaning is **not established** — the test pins the bytes
/// (`09 04 00 00`) as bytes and asserts the reader exposes the leaf only as
/// an uninterpreted leaf: no string, no block, no semantic name.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f12_g_retail_type_255_leaf_is_four_uninterpreted_bytes() {
    let dir = f12_g_game_dir();
    let image = f12_g_file(&dir, "strings.dll");
    let resources = f12_g_resources(&image, "strings.dll");

    let leaf = f12_g_leaf(&resources, [255, 1, 1033]);
    assert_eq!(leaf.data.directory_offset, 0x15a8);
    assert_eq!(leaf.data.rva, 0x1_d674);
    assert_eq!(leaf.data.file_offset, 0x1_d674);
    assert_eq!(leaf.data.size, 4);
    assert_eq!(leaf.data.code_page, 1252);
    assert_eq!(leaf.data.reserved, 0);

    let payload = &image
        [leaf.data.file_offset as usize..leaf.data.file_offset as usize + leaf.data.size as usize];
    assert_eq!(payload, [0x09, 0x04, 0x00, 0x00].as_slice());
    assert_eq!(
        cs_assets::install::sha256(payload).to_hex(),
        F12_G_TYPE_255_SHA256
    );

    // The reader exposes it as a leaf and nothing else: it is not a string
    // block (its payload contributes no `StringUnit`), it is not reached by
    // `string_block`, and no `PeResources` surface decodes it. The workspace
    // scan in `accept_f12_g_no_engine_path_reads_the_type_255_payload` pins
    // the other half: no production source names or interprets the payload.
    assert!(
        resources
            .strings()
            .iter()
            .all(|block| block.data.file_offset != leaf.data.file_offset),
        "the type-255 payload must never be decoded as a string block"
    );
    assert!(!f12_g_string_leaf(leaf));
    assert_eq!(
        resources.leaves().len() - resources.strings().len(),
        2,
        "the type-16 and type-255 leaves are the whole non-string remainder"
    );
}

/// The `.H` resource ids correlate with `langui.dll`'s string-table blocks —
/// every number of the finding's correlation tables, re-derived through
/// `read_resource_header` (which reads the members out of `crimson.rof`
/// through `read_tree`/`read_member`) and `read_pe_resources`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f12_g_retail_header_ids_correlate_with_langui_blocks() {
    let dir = f12_g_game_dir();
    let rof = f12_g_file(&dir, CRIMSON_ROF);

    let resource_h = f12_g_rof_member(&rof, "ASSETS/SCRIPTS/RESOURCE.H");
    let resrc1_h = f12_g_rof_member(&rof, "ASSETS/SCRIPTS/RESRC1.H");
    assert_eq!(resource_h.len(), 29_579);
    assert_eq!(
        cs_assets::install::sha256(&resource_h).to_hex(),
        F12_G_RESOURCE_H_SHA256
    );
    assert_eq!(resrc1_h.len(), 8_922);
    assert_eq!(
        cs_assets::install::sha256(&resrc1_h).to_hex(),
        F12_G_RESRC1_H_SHA256
    );

    let mut context = ParseContext::with_defaults("RESOURCE.H");
    let resource_h = read_resource_header(&mut context, &resource_h)
        .expect("RESOURCE.H reads as a resource header");
    let mut context = ParseContext::with_defaults("RESRC1.H");
    let resrc1_h =
        read_resource_header(&mut context, &resrc1_h).expect("RESRC1.H reads as a resource header");

    // The recorded per-header counts.
    assert_eq!(resource_h.defines().count(), 635);
    assert_eq!(resrc1_h.defines().count(), 185);
    let resource_ids = f12_g_define_ids(&resource_h, false);
    let resrc1_ids = f12_g_define_ids(&resrc1_h, false);
    assert_eq!(resource_ids.len(), 612);
    assert_eq!(resrc1_ids.len(), 173);
    assert_eq!(resource_ids.iter().copied().min(), Some(9));
    assert_eq!(resource_ids.iter().copied().max(), Some(40_001));
    assert_eq!(resrc1_ids.iter().copied().min(), Some(101));
    assert_eq!(resrc1_ids.iter().copied().max(), Some(40_170));
    let all_ids: BTreeSet<u32> = resource_ids.union(&resrc1_ids).copied().collect();
    assert_eq!(all_ids.len(), 782, "the two headers' distinct id space");

    let resource_string_ids = f12_g_define_ids(&resource_h, true);
    let resrc1_string_ids = f12_g_define_ids(&resrc1_h, true);
    assert_eq!(resource_string_ids.len(), 346);
    assert_eq!(resrc1_string_ids.len(), 171);
    // RESRC1.H's string ids are the recorded contiguous run.
    assert_eq!(
        resrc1_string_ids,
        (40_000u32..=40_170).collect::<BTreeSet<_>>(),
        "the 171 string ids are exactly 40 000..=40 170"
    );

    // The measured per-image scores: every surveyed image, its block count,
    // and how many distinct header values name one of its blocks under the
    // `(block - 1) * 16 + index` numbering. `langui.dll` wins outright;
    // `strings.dll` scores 123 and the next-best image (SETUPENU.DLL) 169.
    let mut langui_blocks = BTreeSet::new();
    let mut langui = None;
    for (spelling, blocks, score) in F12_G_IMAGE_SCORES {
        let image = f12_g_file(&dir, spelling);
        let resources = f12_g_resources(&image, spelling);
        let block_ids = f12_g_block_ids(&resources);
        assert_eq!(block_ids.len(), *blocks, "{spelling}");
        let hits = all_ids
            .iter()
            .filter(|id| block_ids.contains(&((*id / 16 + 1) as u16)))
            .count();
        assert_eq!(hits, *score, "{spelling}");
        if *spelling == "GOSDATA/ASSETS/BINARIES/langui.dll" {
            langui = Some(resources);
            langui_blocks = block_ids;
        }
    }
    let langui = langui.expect("the table lists langui.dll");

    // The four-cell A/B × scope table for langui.dll: scoring all 820
    // defines (duplicates included) and only the 532 string-named ones.
    let score = |defines: &mut dyn Iterator<Item = u32>, offset: u32| {
        defines
            .filter(|id| langui_blocks.contains(&(((*id + 16 * offset) / 16) as u16)))
            .count()
    };
    let all_defines = || {
        resource_h
            .defines()
            .chain(resrc1_h.defines())
            .filter_map(|define| define.resource_id())
    };
    let string_defines = || {
        resource_h
            .defines()
            .chain(resrc1_h.defines())
            .filter(|define| f12_g_string_name(define.name))
            .filter_map(|define| define.resource_id())
    };
    assert_eq!(all_defines().count(), 820);
    assert_eq!(string_defines().count(), 532);
    // `(v + 16) / 16` is `v / 16 + 1` for the ids in range; `(v) / 16` is the
    // 0-based alternative the measurement rejects.
    assert_eq!(
        score(&mut all_defines(), 1),
        813,
        "B numbering, all defines"
    );
    assert_eq!(
        score(&mut all_defines(), 0),
        705,
        "A numbering, all defines"
    );
    assert_eq!(score(&mut string_defines(), 1), 525, "B, string names");
    assert_eq!(score(&mut string_defines(), 0), 463, "A, string names");

    // The seven values no block answers, and the eighteen blocks no value
    // names.
    let misses: Vec<u32> = all_ids
        .iter()
        .copied()
        .filter(|id| !langui_blocks.contains(&((*id / 16 + 1) as u16)))
        .collect();
    assert_eq!(misses, F12_G_HEADER_MISSES);
    let named: BTreeSet<u16> = all_ids.iter().map(|id| (*id / 16 + 1) as u16).collect();
    let unaddressed: Vec<u16> = langui_blocks.difference(&named).copied().collect();
    assert_eq!(unaddressed, F12_G_UNADDRESSED_BLOCKS);

    // The boundary argument: RESRC1.H's run 40 000..=40 170 needs blocks
    // 2501..=2511 under the `+ 1` numbering — all present, each a full
    // sixteen-unit block — while the 0-based numbering would need block 2500,
    // which does not exist.
    for block_id in 2501u16..=2511 {
        let block = langui
            .string_block(block_id)
            .unwrap_or_else(|| panic!("block {block_id} must exist"));
        assert_eq!(block.units.len(), 16, "block {block_id} is full");
    }
    assert!(
        langui.string_block(2500).is_none(),
        "the 0-based numbering would need block 2500; it does not exist"
    );
    assert_eq!(
        langui_blocks.iter().copied().max(),
        Some(2511),
        "block 2511 is the last block the image carries"
    );

    // The tail agreement: blocks 2501..=2511 hold eleven empty units. Five are
    // the ids RESRC1.H omits (40 171..=40 175, all in block 2511); the other
    // six are all ids the header does name.
    let mut empty: Vec<u32> = Vec::new();
    for block_id in 2501u16..=2511 {
        let block = langui.string_block(block_id).expect("present");
        for unit in &block.units {
            if unit.code_units.is_empty() {
                empty.push(unit.id);
            }
        }
    }
    empty.sort_unstable();
    let omitted: Vec<u32> = (40_171..=40_175).collect();
    let named_empty: Vec<u32> = F12_G_NAMED_EMPTY_UNITS.to_vec();
    assert_eq!(
        empty,
        [named_empty.clone(), omitted.clone()].concat(),
        "eleven empty units: the six the header names, then the five it omits"
    );
    for id in &named_empty {
        assert!(
            resrc1_string_ids.contains(id),
            "empty unit {id} is an id the header names"
        );
    }
    for id in &omitted {
        assert!(
            !resrc1_string_ids.contains(id),
            "empty unit {id} is an id the header omits"
        );
    }
}

/// An authored image with a type-16 leaf and a type-255 leaf next to a string
/// block: the reader must keep both as leaves and decode neither — the same
/// retention the retail tests count on, exercised on synthetic bytes so CI
/// sees it.
#[test]
fn accept_f12_g_non_string_leaves_are_retained_uninterpreted() {
    let mut rsrc = Rsrc::default();
    // One three-level path per type, all hung off one root: `[type, 1, 1033]`.
    let root = rsrc.dir(3);
    let mut language_dirs = Vec::new();
    for (index, type_id) in [RT_STRING, 16, 255].into_iter().enumerate() {
        rsrc.id(root, index, type_id);
        let names = rsrc.dir(1);
        rsrc.id(names, 0, 1);
        let languages = rsrc.dir(1);
        rsrc.id(languages, 0, LANG_ENGLISH_US);
        rsrc.sub(names, 0, languages);
        rsrc.sub(root, index, names);
        language_dirs.push(languages);
    }
    let entry = rsrc.leaf_here(&Rsrc::string_payload(&["alpha"]), 1252);
    rsrc.data_entry(language_dirs[0], 0, entry);
    // The two non-string leaves: four-byte authored payloads (not the
    // original bytes — the fixture pins *handling*, not the recorded value).
    let entry = rsrc.leaf_here(b"\xde\xad\xbe\xef", 1252);
    rsrc.data_entry(language_dirs[1], 0, entry);
    let entry = rsrc.leaf_here(&[0xaa, 0xbb, 0xcc, 0xdd], 1252);
    rsrc.data_entry(language_dirs[2], 0, entry);

    let bytes = PeBuilder::new().with_rsrc(0x2000, rsrc.finish()).build();
    let resources = resources(&bytes);

    assert_eq!(resources.strings().len(), 1);
    assert_eq!(resources.leaves().len(), 3);
    let other: Vec<Vec<u32>> = resources
        .leaves()
        .iter()
        .filter(|leaf| !f12_g_string_leaf(leaf))
        .map(|leaf| leaf.path.iter().filter_map(ResourceKey::id).collect())
        .collect();
    assert_eq!(other, vec![vec![16, 1, 1033], vec![255, 1, 1033]]);
    for leaf in resources.leaves() {
        if f12_g_string_leaf(leaf) {
            continue;
        }
        assert_eq!(leaf.data.code_page, 1252);
        assert_eq!(leaf.data.size, 4);
        // The leaf exposes its data entry only: there is no decoded view of
        // the payload, and no `StringUnit` was produced from it.
        assert!(
            resources
                .strings()
                .iter()
                .all(|block| block.data.file_offset != leaf.data.file_offset)
        );
    }
}

/// **No engine path reads the type-255 payload.** The leaf's only identity is
/// its path `[255, 1, 1033]` and its data-entry span; a later stage that
/// starts guessing a meaning must touch production source to do it, and the
/// plausible ways to do so — selecting on resource type `255`, hard-coding
/// the recorded span or spelling the payload — are exactly what this scan
/// refuses. If a legitimate change trips a needle, the needle's name is the
/// documented leaf: explain the new consumer or pick a spelling that cannot
/// be mistaken for one, do not weaken the scan.
#[test]
fn accept_f12_g_no_engine_path_reads_the_type_255_payload() {
    // Spellings of the leaf's recorded identity: its file offset and its four
    // payload bytes. These must appear nowhere outside this test file —
    // anywhere else is a consumer of the measurement, not a reader of data.
    const SPAN_OR_PAYLOAD: &[&str] = &[
        "0x1d674",
        "0x1D674",
        "09040000",
        "0x09040000",
        "09 04 00 00",
        "09, 04, 00, 00",
    ];
    // Ways to single out the leaf's type in a source file that already
    // touches the PE resource tree.
    const TYPE_255_SELECTORS: &[&str] = &[
        "Id(255)",
        "Some(255)",
        "== 255",
        "RT_255",
        "type_255",
        "type-255",
    ];
    // A file that never names the resource-leaf surface cannot be reading the
    // leaf's payload through it.
    const LEAF_API: &[&str] = &[
        "ResourceKey",
        "ResourceLeaf",
        "read_pe_resources",
        "pe_resources",
        "string_leaf",
        "other_leaves",
        "leaf.data",
    ];

    let crates_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate lives under crates/")
        .to_path_buf();
    let workspace = crates_dir.parent().expect("a workspace root").to_path_buf();

    // The resource reader itself names the type once, in its module
    // documentation ("an unassigned type `255`"). Any code touching the leaf
    // — a selector, a constant, a match arm — adds a second `255` and fails
    // here. If the reader legitimately gains another `255`, explain it in
    // this count, never silently.
    let reader = crates_dir.join("cs_formats/src/pe_resources.rs");
    let text = std::fs::read_to_string(&reader)
        .unwrap_or_else(|error| panic!("reading {}: {error}", reader.display()));
    assert_eq!(
        text.matches("255").count(),
        1,
        "pe_resources.rs must mention type 255 only in the module doc that \
         records it as unassigned; a second occurrence is the reader touching \
         the leaf"
    );
    let mut sources = Vec::new();
    for group in [&crates_dir, &workspace.join("tools")] {
        let mut pending = vec![group.to_path_buf()];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir)
                .unwrap_or_else(|error| panic!("reading {}: {error}", dir.display()))
            {
                let path = entry.expect("a directory entry").path();
                if path.is_dir() {
                    pending.push(path);
                } else if path.extension() == Some(std::ffi::OsStr::new("rs")) {
                    sources.push(path);
                }
            }
        }
    }
    assert!(!sources.is_empty(), "the workspace sources must be found");

    let mut checked = 0usize;
    for source in &sources {
        // Test code may name the leaf — pinning it is the tests' job. The
        // scan covers production files only: anything under a `tests/`
        // directory or in a file whose name carries `test`.
        let is_test = source.components().any(|part| part.as_os_str() == "tests")
            || source
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains("test"));
        if is_test {
            continue;
        }
        checked += 1;
        let text = std::fs::read_to_string(source)
            .unwrap_or_else(|error| panic!("reading {}: {error}", source.display()));
        for needle in SPAN_OR_PAYLOAD {
            assert!(
                !text.contains(needle),
                "{} spells the type-255 leaf's span or payload ({needle}): \
                 that is a consumer of the measurement, and the payload's \
                 meaning is unestablished — see the F12-G finding",
                source.display()
            );
        }
        if LEAF_API.iter().any(|marker| text.contains(marker)) {
            for needle in TYPE_255_SELECTORS {
                assert!(
                    !text.contains(needle),
                    "{} selects resource type 255 ({needle}) while touching the \
                     leaf API: the leaf must stay an uninterpreted leaf — see \
                     the F12-G finding",
                    source.display()
                );
            }
        }
    }
    assert!(
        checked > 0,
        "the scan found no production sources — the walk is broken, not clean"
    );
    println!("f12-g tripwire: {checked} production sources scanned clean");
}
