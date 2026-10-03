//! Shared measurement support for the F27-E.1 acceptance tests and its evidence
//! harness (task #547, "Bind the original ammunition damage amounts and
//! per-airframe gun mounts once they are measurable",
//! `specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`, F27
//! non-negotiable 1 and 2).
//!
//! It is a `tests/` subdirectory module, not a test target of its own: it has no
//! `#[test]`, and cargo never discovers `tests/<dir>/mod.rs` as a binary. Both
//! `accept_f27_e_1_retail_measurability.rs` and `evidence_report_f27_e_1.rs`
//! include it with `#[path = "f27_e_1_support/mod.rs"]`.
//!
//! # What this module measures
//!
//! This stage was handed a claim it cannot resolve: the original's per-type
//! damage amounts and the per-airframe gun-group assignment are in the
//! executable, and no shipped file declares them. Before recording that as a
//! deferral the stage has to *measure* it rather than repeat F27-E's argument,
//! so this module reads, through production readers only:
//!
//! * `GOSDATA/ASSETS/BINARIES/langui.dll` — the shipped UI language image whose
//!   `RT_STRING` blocks hold the ammunition and gun rows the loadout screens ask
//!   the engine for, read through `cs_formats::pe_resources`;
//! * `ASSETS/SCRIPTS/RESOURCE.H` and `ASSETS/SCRIPTS/RESRC1.H` — the original
//!   build's own two generated headers, read through `cs_assets`' ROF mount and
//!   `cs_formats::text::read_resource_header`;
//! * `crimson.exe` and `crimson.icd` — the two images that would hold the
//!   tables, read as inert PE bytes through `cs_formats::pe_resources`;
//! * `ZBD/planes.zbd` — the shared aircraft geometry, read through
//!   `cs_formats::gamez::read_gamez_nodes`.
//!
//! # What deliberately never leaves this file
//!
//! **No original display text is returned, printed or committed.** Every row
//! this module reads is reduced to a *property* of it: its string id, its length
//! in code units, whether it carries an ASCII digit, and which digits. The
//! prose the original wrote about a type is read by the acceptance test and
//! asserted only on those properties, so `AGENTS.md` rule 3 is satisfied by not
//! returning the bytes at all rather than by filtering them afterwards. The one
//! thing that *is* returned is the original's own **identifier** vocabulary —
//! `IDS_*` macro names and declared ids — which is what F27-D already committed
//! and which says nothing localizable.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use cs_assets::install::{self, content_fingerprint, fingerprint, sha256};
use cs_assets::rof::mount_rof_into;
use cs_assets::vfs::{INSTALL_NAMESPACE, MountBuilder, SessionBuilder};
use cs_content::weapons::{ORIGINAL_AMMO_NAME_BLOCKS, ORIGINAL_GUN_GROUPS};
use cs_formats::text::read_resource_header;
use cs_types::asset_id::{AssetKey, MountId, MountNamespace, PrecedenceClass, ResolveContext};
use cs_types::evidence::ContentHash;

/// The retail container that holds the two generated headers.
pub const BASE_CONTAINER: &str = "GOSDATA/ASSETS/crimson.rof";

/// The engine's own generated header, a C include the original build produced.
///
/// The spelling is the production constant's, so this module cannot read a
/// different member than the schema names.
pub const RESOURCE_HEADER: &str = cs_content::weapons::ORIGINAL_RESOURCE_HEADER;

/// The original build's second generated header.
pub const RESOURCE_HEADER_SECOND: &str = "ASSETS/SCRIPTS/RESRC1.H";

/// The members this stage reads out of the retail container.
pub const MEMBERS: [&str; 2] = [RESOURCE_HEADER, RESOURCE_HEADER_SECOND];

/// The shipped UI language image, relative to the installation root.
pub const LANGUI_DLL: &str = "GOSDATA/ASSETS/BINARIES/langui.dll";

/// The executable that would hold the per-type damage table.
pub const CRIMSON_EXE: &str = "crimson.exe";

/// The second MZ image beside the executable.
pub const CRIMSON_ICD: &str = "crimson.icd";

/// The shared aircraft geometry container, relative to the installation root.
pub const PLANES_ZBD: &str = "ZBD/planes.zbd";

/// The language the measured rows carry (F12-B's survey: `1033`, `0x0409`,
/// en-US, in all three surveyed images).
pub const ENGLISH_US: u32 = 1033;

/// The macros whose declared ids are the ammunition and gun row blocks, as
/// `(macro, what the block is for)`.
///
/// The ids themselves are **not** written here: the tests read them out of the
/// installation's own header through the production header reader, so a stale
/// constant fails rather than passing.
pub const MEASURED_BLOCK_MACROS: [(&str, &str); 6] = [
    ("IDS_AMMOLONGNAME", "the four ammunition type names"),
    ("IDS_AMMOSHORTNAME", "the four ammunition type short names"),
    ("IDS_AMMOABBRNAME", "the four ammunition type abbreviations"),
    (
        "IDS_AMMODESCRIPTION",
        "the four ammunition type descriptions",
    ),
    ("IDS_GUNLONGNAME", "the five gun names"),
    ("IDS_GUNSHORTNAME", "the five gun caliber rows"),
];

/// The words a `#define` name must contain to be a candidate weapon,
/// ammunition or armor constant. Every match in either header is reported, so a
/// future extraction that finds one fails this stage's test instead of passing
/// it.
pub const BALLISTIC_WORDS: [&str; 11] = [
    "damage", "caliber", "calibre", "pierc", "ricoch", "penetr", "bullet", "armor", "armour",
    "gun", "ammo",
];

/// The words that would make a matched identifier a **quantity** rather than a
/// name. No `#define` in either header may carry one.
pub const AMOUNT_WORDS: [&str; 7] = [
    "damage", "caliber", "calibre", "pierc", "ricoch", "penetr", "bullet",
];

/// The schema's own label for the ammunition description block.
pub const AMMUNITION_DESCRIPTION_BLOCK: &str = "ammo_description";

/// The words a second MZ image is searched for, case-insensitively. The original
/// would spell a damage table with at least one of them.
pub const IMAGE_KEYWORDS: [&str; 11] = [
    "caliber", "calibre", "ammo", "armor", "armour", "piercing", "incend", "damage", "rocket",
    "shell", "heat",
];

/// Where each remaining `f27.d.limit.*` claim was re-filed by this stage.
///
/// The table is the stage's **accounting**, not a resolution: each entry names
/// where the open question now lives. The acceptance test refuses a report in
/// which one of the five claims is not covered by a row of this table, so a
/// deferral cannot quietly disappear.
pub const REFILED: [(&str, &str); 5] = [
    (
        "f27.d.limit.ammo_names_damage",
        "#547 F27-E.1 re-filed to #358 REF-OWNER-FIRST-CAPTURE (owner-supplied original run; \
         capture protocol #357) and to #549 F27-E.2, the one agent-reachable route: recover the \
         tables from the protected image, or record which static routes were tried and why each \
         fails",
    ),
    (
        "f27.d.limit.gun_group_assignment",
        "#547 F27-E.1 re-filed to #358 REF-OWNER-FIRST-CAPTURE for the per-airframe gun tables, \
         and to #550 F27-E.3 for the four armor positions the original's own armor screen names \
         (a measured lead, not a resolution)",
    ),
    (
        "f27.d.limit.convergence",
        "#547 F27-E.1 re-filed to #358 REF-OWNER-FIRST-CAPTURE: convergence is original behavior \
         and no shipped file declares it",
    ),
    (
        "f27.d.limit.inheritance",
        "#547 F27-E.1 re-filed to #358 REF-OWNER-FIRST-CAPTURE: inherited velocity is original \
         behavior and no shipped file declares it",
    ),
    (
        "f27.d.limit.interaction_rules",
        "#547 F27-E.1 re-filed to #358 REF-OWNER-FIRST-CAPTURE: penetration, ricochet and \
         in-flight ammo switching are original behavior, and a model for any of them would be \
         invented",
    ),
];

/// The read-only installation root, or a loud failure: a retail test must fail,
/// not pass, when `CS_GAME_DIR` is absent.
pub fn game_dir() -> PathBuf {
    let dir = std::env::var_os("CS_GAME_DIR").expect(
        "CS_GAME_DIR is not set: this measurement needs the original installation \
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

/// The installation's manifest, discovered once: production discovery hashes
/// every file, so every span below is bound to one installation digest.
pub fn installation(root: &Path) -> cs_types::install::InstallManifest {
    static CACHE: OnceLock<cs_types::install::InstallManifest> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            install::discover(root)
                .unwrap_or_else(|error| panic!("the installation must be discoverable: {error:?}"))
                .manifest
        })
        .clone()
}

/// The installation digest (`install_sha256`) and content digest
/// (`content_sha256`) the evidence record requires, both measured here.
pub fn installation_digests(root: &Path) -> (String, String) {
    let manifest = installation(root);
    (
        fingerprint(&manifest).to_hex(),
        content_fingerprint(&manifest).to_hex(),
    )
}

/// The installation digest every span in this stage is bound to.
pub fn install_sha256(root: &Path) -> ContentHash {
    fingerprint(&installation(root))
}

/// Opens one retail member's bytes through the production ROF mount.
///
/// The mount answers *resolution* and the [`RofSource`] answers *bytes*: a ROF
/// member lives inside its container and may be compressed, so the session's
/// directory read reports `no backing` for it by design and the production member
/// reader is the byte path. The session is still used, to prove the member
/// resolves through the mount before a byte is read.
pub fn read_member(root: &Path, spelling: &str) -> Vec<u8> {
    let install = install_sha256(root);
    let id: String = format!("rof-{}", BASE_CONTAINER.to_ascii_lowercase())
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '.' {
                character
            } else {
                '-'
            }
        })
        .collect();
    let mut builder = SessionBuilder::new(ResolveContext::new(install));
    let source = mount_rof_into(
        &mut builder,
        MountBuilder::new(
            MountId::new(&id).expect("a valid mount id"),
            MountNamespace::new(INSTALL_NAMESPACE).expect("a valid namespace"),
            PrecedenceClass::Shared,
            BASE_CONTAINER,
        )
        .retail(),
        &root.join(BASE_CONTAINER),
    )
    .expect("the base retail archive mounts");
    let session = builder.open();
    let key =
        AssetKey::from_spelling(INSTALL_NAMESPACE, spelling, "default").expect("a valid asset key");
    session
        .resolve(&key)
        .unwrap_or_else(|error| panic!("{spelling}: the member must resolve: {error:?}"));
    source
        .read(&key)
        .unwrap_or_else(|error| panic!("{spelling}: the member must decode: {error:?}"))
        .data
}

/// Reads one loose file of the installation, relative to its root.
///
/// The installation is read-only and these files are not inside a container, so
/// the byte path is the filesystem's; every reader still goes through the
/// production parsers.
pub fn read_loose(root: &Path, relative: &str) -> Vec<u8> {
    let path = root.join(relative);
    std::fs::read(&path)
        .unwrap_or_else(|error| panic!("{relative}: the installation must hold it: {error}"))
}

/// The offset of `needle` inside `haystack`.
pub fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// The shipped `[TAG]` markup code every measured ammunition and caliber row of the
/// language image carries, and which the gun *long* names do not. The code is the
/// production constant's, so this module cannot measure a different code than the
/// schema pins; it is split off and never interpreted.
pub const TEXT_MARKUP: &str = cs_content::weapons::ORIGINAL_TEXT_MARKUP;

/// Splits the shipped markup code off one measured row, leaving the display
/// text. A row with no code is returned unchanged, and a row whose whole content
/// is the code becomes the empty string.
pub fn strip_markup(text: &str) -> &str {
    match text.strip_prefix(TEXT_MARKUP) {
        Some(rest) => rest,
        None => text,
    }
}

/// The ASCII digits of one measured row, in order.
pub fn digits(text: &str) -> Vec<char> {
    text.chars().filter(char::is_ascii_digit).collect()
}

/// One measured row of the language image, reduced to properties.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// The string id the row carries.
    pub id: u32,
    /// The language the row carries.
    pub language: u32,
    /// The row's length in UTF-16 code units, markup included.
    pub code_units: usize,
    /// Whether the row begins with the shipped markup code.
    pub marked: bool,
    /// The ASCII digits the **display text** carries, with the markup split
    /// off.
    pub digits: Vec<char>,
    /// Whether the display text is empty once the markup is split off.
    pub empty: bool,
}

/// The measured properties of every row in one id run of the language image,
/// read through `cs_formats::pe_resources`.
///
/// The text itself never leaves this function: a caller gets lengths and digit
/// sets, which is what "does this row state a number?" is a question about.
pub fn read_rows(root: &Path, first: u32, count: u32) -> Vec<Row> {
    let bytes = read_loose(root, LANGUI_DLL);
    let mut context = cs_formats::ParseContext::with_defaults(LANGUI_DLL);
    let resources = cs_formats::pe_resources::read_pe_resources(&mut context, &bytes)
        .unwrap_or_else(|error| panic!("{LANGUI_DLL}: the string image must read: {error}"));
    (first..first + count)
        .map(|id| {
            let unit = resources
                .string_unit(id, Some(ENGLISH_US))
                .unwrap_or_else(|| panic!("{LANGUI_DLL}: string id {id} must be present"));
            // `(block - 1) * 16 + index` is the numbering the production reader
            // documents, so the block that holds an id is `id / 16 + 1`; the
            // language is a property of the block, not of the unit.
            let block = (id / 16 + 1) as u16;
            let language = resources
                .string_block(block)
                .unwrap_or_else(|| panic!("{LANGUI_DLL}: string block {block} must be present"))
                .language;
            let text = unit.text.clone().unwrap_or_default();
            Row {
                id,
                language,
                code_units: unit.code_units.len(),
                marked: text.starts_with(TEXT_MARKUP),
                digits: digits(strip_markup(&text)),
                empty: strip_markup(&text).trim().is_empty(),
            }
        })
        .collect()
}

/// The id the installation's own header declares for one macro, read through the
/// production header reader.
pub fn declared_id(header_bytes: &[u8], macro_name: &str) -> u32 {
    let mut context = cs_formats::ParseContext::with_defaults(RESOURCE_HEADER);
    let header = read_resource_header(&mut context, header_bytes)
        .unwrap_or_else(|error| panic!("{RESOURCE_HEADER}: the header must read: {error}"));
    match header.lookup(macro_name.as_bytes()) {
        cs_formats::text::HeaderLookup::Found(define) => define
            .resource_id()
            .unwrap_or_else(|| panic!("{RESOURCE_HEADER}: {macro_name} must declare an id")),
        cs_formats::text::HeaderLookup::Missing => {
            panic!("{RESOURCE_HEADER}: {macro_name} must be declared")
        }
        cs_formats::text::HeaderLookup::Ambiguous(count) => {
            panic!("{RESOURCE_HEADER}: {macro_name} is declared {count} times")
        }
    }
}

/// One `#define` of either generated header, as `(macro name, value text)`.
pub fn defines(root: &Path, member: &str) -> Vec<(String, String)> {
    let bytes = read_member(root, member);
    let mut context = cs_formats::ParseContext::with_defaults(member);
    let header = read_resource_header(&mut context, &bytes)
        .unwrap_or_else(|error| panic!("{member}: the header must read: {error}"));
    header
        .defines()
        .map(|define| {
            (
                String::from_utf8_lossy(define.name).into_owned(),
                String::from_utf8_lossy(define.value.text()).into_owned(),
            )
        })
        .collect()
}

/// The `#define` name the original build spells one declared gun group with:
/// the group's own identifier plus the header's `IDS_` prefix.
///
/// **Measured**: `RESOURCE.H` declares `IDS_INNERWINGGUNS 3061` and its
/// nineteen siblings, and the ids the schema pins are those values. The prefix
/// is the header's own convention and is not a guess about a name.
#[must_use]
pub fn gun_group_macro(group: &cs_content::weapons::DeclaredGunGroup) -> String {
    format!("IDS_{}", group.label())
}

/// The ammunition description block's base id and row count, as declared by the
/// installation's own header and pinned by the schema's own block table.
#[must_use]
pub fn ammunition_description_block(root: &Path) -> (u32, u32) {
    let header = read_member(root, RESOURCE_HEADER);
    let base = declared_id(&header, "IDS_AMMODESCRIPTION");
    let schema_base = ORIGINAL_AMMO_NAME_BLOCKS
        .iter()
        .find(|(_, block)| *block == AMMUNITION_DESCRIPTION_BLOCK)
        .map(|(id, _)| *id)
        .expect("the schema carries the ammunition description block");
    assert_eq!(
        base, schema_base,
        "the header's declared description base and the schema's must agree"
    );
    (base, cs_content::weapons::ORIGINAL_AMMUNITION_TYPES)
}

/// The gun caliber block's base id and row count, as declared by the
/// installation's own header and the schema's gun count.
#[must_use]
pub fn gun_caliber_block(root: &Path) -> (u32, u32) {
    let header = read_member(root, RESOURCE_HEADER);
    (
        declared_id(&header, "IDS_GUNSHORTNAME"),
        cs_content::weapons::ORIGINAL_SELECTABLE_GUNS,
    )
}

/// One section of one PE image, as measured.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    /// The section name without its `NUL` padding.
    pub name: String,
    /// `SizeOfRawData`: the bytes the section has on disk.
    pub raw_size: u32,
    /// The Shannon entropy of those bytes, in bits per byte.
    pub entropy: f64,
    /// How many of those bytes are zero.
    pub zero_bytes: u64,
}

/// One PE image, as measured.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    /// The container's spelling.
    pub name: String,
    /// The file's length in bytes.
    pub len: usize,
    /// Whether the file starts with the `MZ` signature.
    pub is_mz: bool,
    /// Its sections, in the section table's own order.
    pub sections: Vec<Section>,
    /// The size of `IMAGE_DIRECTORY_ENTRY_RESOURCE`, when the image declares
    /// one.
    pub resource_directory_size: Option<u32>,
    /// The resource **types** the directory actually holds, at depth one.
    pub resource_types: Vec<u32>,
}

/// The Shannon entropy of `bytes`, in bits per byte.
///
/// Computed here rather than through a production reader because no production
/// reader measures entropy; the *bytes* are production-read, which is what the
/// measurement is about.
#[must_use]
pub fn entropy(bytes: &[u8]) -> f64 {
    if bytes.is_empty() {
        return 0.0;
    }
    let mut counts = [0_u64; 256];
    for byte in bytes {
        counts[usize::from(*byte)] += 1;
    }
    let total = bytes.len() as f64;
    -counts
        .iter()
        .filter(|count| **count > 0)
        .map(|count| {
            let share = *count as f64 / total;
            share * share.log2()
        })
        .sum::<f64>()
}

/// Reads one PE image and measures it.
pub fn read_image(root: &Path, relative: &str) -> Image {
    let bytes = read_loose(root, relative);
    let mut context = cs_formats::ParseContext::with_defaults(relative);
    let layout = cs_formats::pe_resources::read_pe_layout(&mut context, &bytes)
        .unwrap_or_else(|error| panic!("{relative}: the PE layout must read: {error}"));
    let sections = layout
        .sections()
        .iter()
        .map(|section| {
            let start = section.raw_pointer as usize;
            let end = start.saturating_add(section.raw_size as usize);
            let slice = if end <= bytes.len() {
                &bytes[start..end]
            } else {
                panic!("{relative}: section {} runs past the file", section.name)
            };
            Section {
                name: section.name.clone(),
                raw_size: section.raw_size,
                entropy: entropy(slice),
                zero_bytes: slice.iter().filter(|byte| **byte == 0).count() as u64,
            }
        })
        .collect();
    let resource_directory_size = layout
        .resource_directory()
        .filter(|directory| directory.size > 0)
        .map(|directory| directory.size);
    // The depth-one keys are the resource *types*. Reading them is a separate
    // parse: a protected image may have a readable layout and an unreadable
    // directory, and that difference is itself part of the measurement.
    let resource_types = cs_formats::pe_resources::read_pe_resources(&mut context, &bytes)
        .map(|resources| {
            let mut types: Vec<u32> = resources
                .leaves()
                .iter()
                .filter_map(|leaf| {
                    leaf.key(0)
                        .and_then(cs_formats::pe_resources::ResourceKey::id)
                })
                .collect();
            types.sort_unstable();
            types.dedup();
            types
        })
        .unwrap_or_default();
    Image {
        name: relative.to_owned(),
        len: bytes.len(),
        is_mz: bytes.starts_with(b"MZ"),
        sections,
        resource_directory_size,
        resource_types,
    }
}

/// The keywords one image's bytes carry, counted case-insensitively.
pub fn keyword_counts(root: &Path, relative: &str) -> Vec<(String, u64)> {
    let bytes = read_loose(root, relative).to_ascii_lowercase();
    IMAGE_KEYWORDS
        .iter()
        .map(|keyword| {
            let needle = keyword.to_ascii_lowercase();
            let count = bytes
                .windows(needle.len())
                .filter(|window| *window == needle.as_bytes())
                .count() as u64;
            ((*keyword).to_owned(), count)
        })
        .collect()
}

/// One measured plane-geometry node-name census.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeCensus {
    /// The container's spelling.
    pub container: String,
    /// `SizeOfRawData` of every node record, unchanged.
    pub nodes: usize,
    /// How many distinct node names the container holds.
    pub distinct_names: usize,
    /// The distinct names that mention a gun, in sorted order.
    pub gun_bearing: Vec<String>,
    /// The declared gun-group labels that appear **anywhere inside** a node name,
    /// in sorted order.
    ///
    /// The test is a substring test, not an equality test, and that is the
    /// conservative direction for the claim this stage makes: if no node name
    /// even *contains* a group's label, then certainly no node name *is* one.
    pub declared_groups_present: Vec<String>,
}

/// Reads `ZBD/planes.zbd` through the production node reader and censuses its
/// node names against the twenty declared gun groups.
pub fn read_node_census(root: &Path) -> NodeCensus {
    let bytes = read_loose(root, PLANES_ZBD);
    let mut context = cs_formats::ParseContext::with_defaults(PLANES_ZBD);
    let nodes = cs_formats::gamez::read_gamez_nodes(&mut context, &bytes)
        .unwrap_or_else(|error| panic!("{PLANES_ZBD}: the node array must read: {error}"));
    let mut names: Vec<String> = nodes.nodes.iter().map(|node| node.name.clone()).collect();
    names.sort();
    names.dedup();
    let gun_bearing = names
        .iter()
        .filter(|name| {
            let lower = name.to_ascii_lowercase();
            lower.contains("gun") || lower.contains("turret")
        })
        .cloned()
        .collect();
    let declared_groups_present = ORIGINAL_GUN_GROUPS
        .iter()
        .filter(|group| {
            let label = group.label();
            names.iter().any(|name| {
                name.to_ascii_uppercase()
                    .contains(&label.to_ascii_uppercase())
            })
        })
        .map(|group| group.label().to_owned())
        .collect();
    NodeCensus {
        container: PLANES_ZBD.to_owned(),
        nodes: nodes.nodes.len(),
        distinct_names: names.len(),
        gun_bearing,
        declared_groups_present,
    }
}

/// A second observation of the same installation, for the evidence harness.
///
/// Every field is an identifier, a count, a length, a digest or a boolean. The
/// harness's artifact is rendered from this, so the report describes what a
/// production reader actually saw rather than what the acceptance test asserted.
#[derive(Clone, Debug, Default)]
pub struct Observation {
    /// `member -> (decoded bytes, sha256)`, for every member read.
    pub members: Vec<(String, u64, String)>,
    /// `(macro, id)` for every block macro the header declares.
    pub block_ids: Vec<(String, u32)>,
    /// `(macro, id)` for the twenty gun-group macros.
    pub group_ids: Vec<(String, u32)>,
    /// The measured properties of the four ammunition descriptions.
    pub ammunition_descriptions: Vec<Row>,
    /// The measured properties of the five gun caliber rows.
    pub caliber_rows: Vec<Row>,
    /// Every `#define` of either header whose **name** contains a ballistic
    /// word, with its value text: `(macro, value)`.
    pub ballistic_defines: Vec<(String, String)>,
    /// How many `#define` values in either header spell a decimal point or a
    /// comma, which a damage multiplier would need.
    pub non_integer_values: usize,
    /// The measured `crimson.exe`.
    pub executable: Option<Image>,
    /// The measured `crimson.icd` keyword census.
    pub icd_keywords: Vec<(String, u64)>,
    /// The measured `ZBD/planes.zbd` node-name census.
    pub planes: Option<NodeCensus>,
    /// How many of the twenty declared gun groups a plane node names.
    pub declared_groups_in_planes: usize,
}

/// The re-read. A member the reader cannot open is reported as absent rather
/// than substituted, so the harness records what it saw.
pub fn observe(root: &Path) -> Observation {
    let mut observation = Observation::default();
    for member in MEMBERS {
        let bytes = read_member(root, member);
        observation.members.push((
            (*member).to_owned(),
            bytes.len() as u64,
            sha256(&bytes).to_hex(),
        ));
    }

    let header = read_member(root, RESOURCE_HEADER);
    for (macro_name, _) in MEASURED_BLOCK_MACROS {
        observation
            .block_ids
            .push(((*macro_name).to_owned(), declared_id(&header, macro_name)));
    }
    for group in ORIGINAL_GUN_GROUPS {
        observation
            .group_ids
            .push((group.label().to_owned(), group.id()));
    }

    let (description_base, description_count) = ammunition_description_block(root);
    observation.ammunition_descriptions = read_rows(root, description_base, description_count);
    let (caliber_base, caliber_count) = gun_caliber_block(root);
    observation.caliber_rows = read_rows(root, caliber_base, caliber_count);

    let mut ballistic = Vec::new();
    let mut non_integer = 0_usize;
    for member in MEMBERS {
        for (name, value) in defines(root, member) {
            let lower = name.to_ascii_lowercase();
            if BALLISTIC_WORDS.iter().any(|word| lower.contains(word)) {
                ballistic.push((name.clone(), value.clone()));
            }
            if value.contains('.') || value.contains(',') {
                non_integer += 1;
            }
        }
    }
    observation.ballistic_defines = ballistic;
    observation.non_integer_values = non_integer;

    observation.executable = Some(read_image(root, CRIMSON_EXE));
    observation.icd_keywords = keyword_counts(root, CRIMSON_ICD);
    let census = read_node_census(root);
    observation.declared_groups_in_planes = census.declared_groups_present.len();
    observation.planes = Some(census);
    observation
}
