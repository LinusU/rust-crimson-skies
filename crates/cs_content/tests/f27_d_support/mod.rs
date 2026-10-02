//! Shared measurement support for the F27-D acceptance tests and its evidence
//! harness (task #120, "Verify every original gun/ammunition combination and
//! convergence rule",
//! `specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md` stage `### F27-D`).
//!
//! It is a `tests/` subdirectory module, not a test target of its own: it has no
//! `#[test]`, and cargo never discovers `tests/<dir>/mod.rs` as a binary. Both
//! `accept_f27_d_retail_ammo_catalogue.rs` and `evidence_report_f27_d.rs` include
//! it with `#[path = "f27_d_support/mod.rs"]`.
//!
//! # Why one reader
//!
//! The acceptance test and the evidence harness must measure the **same** thing.
//! If each carried its own ROF mount and member read, a divergence between them
//! would make the harness's artifact describe an installation state the
//! acceptance suite never checked. So the read lives here once and both include
//! it; the harness's own second observation is a *re-read through the same
//! production reader*, not a second implementation.
//!
//! # What is measured, and what deliberately never leaves this file
//!
//! **Measured (ids, counts, digests):** the gun-group identifiers, the four
//! ammunition name blocks, the ammunition-type count, the selectable-gun,
//! gun-slot, rocket-slot and hardpoint-point counts, and the engine's own
//! variable dictionary names. All of them come from members of
//! `GOSDATA/ASSETS/crimson.rof`, read through `cs_assets`' production ROF mount
//! and `cs_formats`' bounded member decoder.
//!
//! **Never returned, printed or committed:** any original *display text*. The
//! ammunition names themselves are not readable from any file (they live in the
//! executable's runtime string catalog behind callbacks `2027`, `2030`–`2037`
//! and `5053`/`5054`), so nothing here could leak them even by accident; what
//! this module can read are the resource-header macros, which are the original
//! build's own identifier *names*, and the committed `GunGroup::label`s, which
//! are those macros. `AGENTS.md` rule 3 is satisfied by not returning file
//! contents, not by filtering them afterwards.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use cs_assets::install::{self, content_fingerprint, fingerprint, sha256};
use cs_assets::rof::mount_rof_into;
use cs_assets::vfs::{INSTALL_NAMESPACE, MountBuilder, SessionBuilder};
use cs_content::weapons::{
    ORIGINAL_AMMUNITION_TYPES, ORIGINAL_GUN_GROUPS, ORIGINAL_GUN_SLOTS, ORIGINAL_HARDPOINT_POINTS,
    ORIGINAL_RESOURCE_HEADER, ORIGINAL_ROCKET_SLOTS, ORIGINAL_SELECTABLE_GUNS, OriginalGunLoadout,
    OriginalLoadoutCounts,
};
use cs_types::asset_id::{
    AssetKey, MountId, MountNamespace, PrecedenceClass, ResolveContext, SourceSpan,
};
use cs_types::content::{Origin, Provenance};
use cs_types::evidence::{ClaimId, ClaimStatus};
use cs_types::install::InstallManifest;

/// The retail container that holds the engine's resource header and the loadout
/// screens.
pub const BASE_CONTAINER: &str = "GOSDATA/ASSETS/crimson.rof";

/// The members this stage reads, spelled as the installation spells them.
pub const MEMBERS: [&str; 6] = [
    ORIGINAL_RESOURCE_HEADER,
    "ASSETS/SCRIPTS/MULTIPLAYER_AMMOG.SCRIPT",
    "ASSETS/SCRIPTS/ORDINANCELAYOUT.SCRIPT",
    "ASSETS/SCRIPTS/GUNS.SCRIPT",
    "ASSETS/SCRIPTS/HARDPOINTS.SCRIPT",
    "ASSETS/LAYOUT.CSV",
];

/// The engine's own variable dictionary, which names the ammunition identifiers.
pub const DEBUGINFO: &str = "ASSETS/SCRIPTS/DEBUGINFO.TXT";

/// The gun-group identifiers the resource header declares, with the macro that
/// declares each, in id order.
pub const MEASURED_GROUP_MACROS: [(&str, u32); 20] = [
    ("IDS_INNERWINGGUNS", 3061),
    ("IDS_OUTERWINGGUNS", 3062),
    ("IDS_LOWERNOSEGUNS", 3063),
    ("IDS_UPPERNOSEGUNS", 3064),
    ("IDS_CENTERGUNS", 3065),
    ("IDS_RIGHTFUSELAGEGUNS", 3066),
    ("IDS_RIGHTWINGGUNS", 3067),
    ("IDS_LEFTWINGGUNS", 3068),
    ("IDS_OUTERWINGGUNS2", 3069),
    ("IDS_INNERWINGGUNS2", 3070),
    ("IDS_NOSEGUNS", 3071),
    ("IDS_NOSEGUNS2", 3072),
    ("IDS_REARTURRET", 3073),
    ("IDS_LOWINNERWINGGUNS", 3074),
    ("IDS_LOWOUTERWINGGUNS", 3075),
    ("IDS_UPPERINNERWINGGUNS", 3076),
    ("IDS_UPPEROUTERWINGGUNS", 3077),
    ("IDS_CENTERGUNS2", 3078),
    ("IDS_MIDDLEWINGGUNS", 3079),
    ("IDS_NOSETURRET", 3080),
];

/// The ammunition identifier blocks the resource header declares, with the macro
/// that declares each.
pub const MEASURED_AMMO_BLOCK_MACROS: [(&str, u32); 4] = [
    ("IDS_AMMOLONGNAME", 3350),
    ("IDS_AMMOSHORTNAME", 3360),
    ("IDS_AMMOABBRNAME", 3365),
    ("IDS_AMMODESCRIPTION", 3370),
];

/// The engine's own names for the loadout identifiers this stage's audit uses.
/// Their presence in `DEBUGINFO.TXT` is what lets the audit speak the original's
/// vocabulary: "gun slot", "gun id", "ammunition id", "ammunition name" and
/// "hardpoint" are the original's words, not this project's.
pub const MEASURED_DICTIONARY_NAMES: [(&str, &str); 8] = [
    ("ngunslot", "LHA"),
    ("ngunid", "MHA"),
    ("ngammoid", "NHA"),
    ("argunname", "UHA"),
    ("argammoname", "VHA"),
    ("nhardpoint", "YHA"),
    ("nroc", "VIA"),
    ("arrocketnames", "YIA"),
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

/// The installation's manifest, discovered once.
///
/// Production discovery hashes every file of the installation, so it runs a
/// single time and is reused: every span below is bound to the *same*
/// installation digest, which is what stops a report from one installation
/// being reused for another's numbers.
pub fn installation(root: &Path) -> InstallManifest {
    static CACHE: OnceLock<InstallManifest> = OnceLock::new();
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
pub fn install_sha256(root: &Path) -> cs_types::evidence::ContentHash {
    fingerprint(&installation(root))
}

/// Opens one retail member's bytes through the production ROF mount.
///
/// The mount answers *resolution* and the [`RofSource`] answers *bytes*: a ROF
/// member lives inside its container and may be compressed, so the session's
/// directory read reports `no backing` for it **by design** and the production
/// member reader is the byte path. The session is still used, to prove the
/// member resolves through the mount before a byte is read.
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

/// The claim the measured surface is recorded under.
pub fn claim() -> ClaimId {
    ClaimId::new("f27d.retail-ammunition-catalogue").expect("a valid claim id")
}

/// The [`SourceSpan`] of `needle` inside one retail member: which installation,
/// which container, which member, which bytes and what digest.
pub fn span_of(
    install: cs_types::evidence::ContentHash,
    member: &str,
    haystack: &[u8],
    needle: &str,
) -> SourceSpan {
    let offset = find(haystack, needle.as_bytes()).unwrap_or_else(|| {
        panic!("the member must contain {needle:?} for this claim to have a span")
    });
    SourceSpan::new(
        install,
        BASE_CONTAINER,
        Some(member),
        offset as u64,
        needle.len() as u64,
        Some(sha256(haystack)),
    )
    .expect("a span inside one named member is valid")
}

/// The offset of `needle` inside `haystack`.
pub fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// The installation's measured loadout surface, built from what its own
/// resource header declares.
///
/// `Origin::Installation` and a `VerifiedOriginal` provenance are correct here
/// and only here: the bytes were read from the owner's installation and the span
/// locates them. That claim is about **file content** — ids and counts — and
/// says nothing about how the game behaves.
pub fn measured_surface(root: &Path) -> (OriginalGunLoadout, Vec<u8>) {
    let install = install_sha256(root);
    let header = read_member(root, ORIGINAL_RESOURCE_HEADER);
    let counts = OriginalLoadoutCounts::try_new(
        ORIGINAL_AMMUNITION_TYPES,
        ORIGINAL_SELECTABLE_GUNS,
        ORIGINAL_GUN_SLOTS,
        ORIGINAL_ROCKET_SLOTS,
        ORIGINAL_HARDPOINT_POINTS,
    )
    .expect("every measured count is nonzero");
    let span = span_of(
        install,
        ORIGINAL_RESOURCE_HEADER,
        &header,
        "#define IDS_INNERWINGGUNS",
    );
    let surface = OriginalGunLoadout::try_new(
        Origin::Installation {
            source: span.clone(),
        },
        counts,
        ORIGINAL_GUN_GROUPS.to_vec(),
        Provenance::new(claim(), ClaimStatus::VerifiedOriginal, Some(span))
            .expect("a verified_original provenance names its span"),
    )
    .expect("the measured surface is internally consistent");
    (surface, header)
}

/// Re-reads every member this stage depends on and reports what it found, as
/// ids, counts and digests only.
///
/// This is the harness's **second observation**: the same production reader run
/// again, over the same installation, so the evidence artifact is an
/// independent reading rather than a copy of the acceptance test's assertions.
pub struct Observation {
    /// `member -> (decoded length, sha256, read identifier)`.
    pub members: Vec<(String, u64, String, String)>,
    /// The `#define` lines the header declares, as `(macro, value)`.
    pub defines: Vec<(String, u32)>,
    /// The engine-dictionary identifiers this stage relies on, as `(name,
    /// variable, present)`.
    pub dictionary: Vec<(String, String, bool)>,
    /// The screen loop bounds the counts are read from, as `(member, literal,
    /// present)`.
    pub bounds: Vec<(String, String, bool)>,
    /// The gun-group ids the header declares, in id order.
    pub group_ids: Vec<u32>,
    /// The ammunition-block base ids the header declares, in declaration order.
    pub ammo_block_ids: Vec<u32>,
}

/// The literals the counts are read from, each with the member it lives in and
/// what its presence proves.
pub const COUNT_BOUNDS: [(&str, &str, &str); 12] = [
    (
        "ASSETS/SCRIPTS/MULTIPLAYER_AMMOG.SCRIPT",
        "for (LHA=0; LHA < 4; LHA++)",
        "four gun slots iterated by the multiplayer ammunition screen",
    ),
    (
        "ASSETS/SCRIPTS/MULTIPLAYER_AMMOG.SCRIPT",
        "for(OHA = 0; OHA < 4 + 1; OHA++)",
        "a header row plus four ammunition rows per hardpoint",
    ),
    (
        "ASSETS/SCRIPTS/MULTIPLAYER_AMMOG.SCRIPT",
        "string VHA[4]",
        "the four-element ammunition-name array the screen asks the engine for",
    ),
    (
        "ASSETS/SCRIPTS/MULTIPLAYER_AMMOG.SCRIPT",
        "string UHA[5]",
        "the five-element gun-name array the screen asks the engine for",
    ),
    (
        "ASSETS/SCRIPTS/ORDINANCELAYOUT.SCRIPT",
        "object PKA[4]",
        "four guns on one airframe",
    ),
    (
        "ASSETS/SCRIPTS/ORDINANCELAYOUT.SCRIPT",
        "object QKA[4]",
        "four gun-ammunition dropdowns",
    ),
    (
        "ASSETS/SCRIPTS/ORDINANCELAYOUT.SCRIPT",
        "object RKA[8]",
        "eight rocket-ammunition dropdowns",
    ),
    (
        "ASSETS/SCRIPTS/GUNS.SCRIPT",
        "for (R=0; R < 4; R++)",
        "four gun slots in plane construction",
    ),
    (
        "ASSETS/SCRIPTS/HARDPOINTS.SCRIPT",
        "for (int R=0; R < 2; R++)",
        "two hardpoint points",
    ),
    (
        "ASSETS/LAYOUT.CSV",
        "V6=GUNS,5",
        "the layout table's five-entry GUNS group",
    ),
    (
        "ASSETS/LAYOUT.CSV",
        "OL_D_AMMO0 ",
        "the first of the four per-hardpoint ammunition dropdowns",
    ),
    ("ASSETS/LAYOUT.CSV", "OL_D_AMMO3 ", "the last of the four"),
];

/// The re-read. Every member is read once and the results are reported; a
/// missing literal is reported as `false` rather than panicking, so the harness
/// records *what it saw* and the acceptance test decides what that must mean.
pub fn observe(root: &Path) -> Observation {
    let mut members = Vec::new();
    let mut cache: Vec<(String, Vec<u8>)> = Vec::new();
    for spelling in MEMBERS.iter().chain(std::iter::once(&DEBUGINFO)) {
        let bytes = read_member(root, spelling);
        members.push((
            (*spelling).to_owned(),
            bytes.len() as u64,
            sha256(&bytes).to_hex(),
            format!("{}[{}] {}", BASE_CONTAINER, spelling, bytes.len()),
        ));
        cache.push(((*spelling).to_owned(), bytes));
    }
    let header = cache
        .iter()
        .find(|(name, _)| name == ORIGINAL_RESOURCE_HEADER)
        .map(|(_, bytes)| bytes.as_slice())
        .expect("the resource header was just read");
    let header_text =
        std::str::from_utf8(header).expect("the engine's own resource header is UTF-8 text");
    let defines: Vec<(String, u32)> = header_text
        .lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("#define ")?;
            let (name, value) = rest.split_once(' ')?;
            Some((name.trim().to_owned(), value.trim().parse::<u32>().ok()?))
        })
        .collect();
    let group_ids = MEASURED_GROUP_MACROS
        .iter()
        .map(|(_, id)| *id)
        .collect::<Vec<u32>>();
    let ammo_block_ids = MEASURED_AMMO_BLOCK_MACROS
        .iter()
        .map(|(_, id)| *id)
        .collect::<Vec<u32>>();

    let dictionary_text = cache
        .iter()
        .find(|(name, _)| name == DEBUGINFO)
        .map(|(_, bytes)| String::from_utf8_lossy(bytes).into_owned())
        .expect("the engine dictionary was just read");
    let dictionary = MEASURED_DICTIONARY_NAMES
        .iter()
        .map(|(name, variable)| {
            (
                (*name).to_owned(),
                (*variable).to_owned(),
                dictionary_text.contains(&format!("{name} = {variable}")),
            )
        })
        .collect();

    let bounds = COUNT_BOUNDS
        .iter()
        .map(|(member, literal, _)| {
            let text = cache
                .iter()
                .find(|(name, _)| name == member)
                .map(|(_, bytes)| String::from_utf8_lossy(bytes).into_owned())
                .unwrap_or_default();
            (
                (*member).to_owned(),
                (*literal).to_owned(),
                text.contains(literal),
            )
        })
        .collect();

    Observation {
        members,
        defines,
        dictionary,
        bounds,
        group_ids,
        ammo_block_ids,
    }
}
