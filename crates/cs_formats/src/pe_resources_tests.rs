//! `accept_f12_b_*`: the bounded, cycle-checked PE resource reader
//! (`crate::pe_resources`).
//!
//! The fixtures are **newly authored** PE images: they follow the public
//! PE/COFF layout, and their ids, languages, code pages and string texts are
//! invented for this test. No original byte, string or resource name of the
//! installation is reproduced. The one test marked
//! `#[ignore = "requires CS_GAME_DIR"]` checks the recorded *structural*
//! facts of the real images (counts, ids, code pages, sizes) instead.

use crate::pe_resources::*;
use crate::text::dialect::{MemberRule, TEXT_DIALECT_INVENTORY, TextDialect, dialect_for_member};
use crate::{ParseContext, ParseErrorKind};

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

    // Block 1 under two languages: en-US and the language id `1` that the
    // surveyed `langui.dll` records.
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
