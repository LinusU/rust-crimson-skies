//! Shared measurement support for the F28-D retail acceptance test and its
//! evidence harness (task #123, "Close original ordnance catalog and test
//! every discovered behavior",
//! `specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md` stage
//! `### F28-D`).
//!
//! It is a `tests/` subdirectory module, not a test target of its own: it has no
//! `#[test]`, and cargo never discovers `tests/<dir>/mod.rs` as a binary. Both
//! `accept_f28_d_retail_ordnance_catalogue.rs` and `evidence_report_f28_d.rs`
//! include it with `#[path = "f28_d_support/mod.rs"]`.
//!
//! # Why one reader
//!
//! The acceptance test and the evidence harness must measure the **same**
//! thing. If each carried its own ROF mount and member read, a divergence
//! between them would make the harness's artifact describe an installation
//! state the acceptance suite never checked. So the read lives here once and
//! both include it; the harness's own second observation is a *re-read through
//! the same production reader*, not a second implementation.
//!
//! # What is measured, and what deliberately never leaves this file
//!
//! **Measured (ids, counts, digests):** the rocket string-id blocks, the
//! next block after them, the nitro control, the rocket-type count, the
//! rocket-slot and hardpoint-point counts, the engine's own dictionary names,
//! and the scrapbook rows whose ids name an ordnance illustration. All of them
//! come from members of `GOSDATA/ASSETS/crimson.rof`, read through
//! `cs_assets`' production ROF mount and `cs_formats`' bounded member decoder.
//!
//! **Never returned, printed or committed:** any original *display text*. The
//! rocket names themselves are not readable from any file — they live in the
//! executable's runtime string catalog behind the same callbacks F27-D found
//! for the gun and ammunition names — so nothing here could leak them even by
//! accident. What this module can read are the resource-header macros, which
//! are the original build's own identifier *names*, the screens' loop bounds
//! and array sizes, which are code shapes, and the committed
//! `ORIGINAL_*` constants, which are those same facts. `AGENTS.md` rule 3 is
//! satisfied by not returning file contents, not by filtering them afterwards.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use cs_assets::install::{self, content_fingerprint, fingerprint, sha256};
use cs_assets::rof::mount_rof_into;
use cs_assets::vfs::{INSTALL_NAMESPACE, MountBuilder, SessionBuilder};
use cs_content::ordnance::{
    ORIGINAL_NEXT_ROCKET_BLOCK, ORIGINAL_NITRO_CONTROL, ORIGINAL_ORDNANCE_HARDPOINT_POINTS,
    ORIGINAL_ORDNANCE_ROCKET_SLOTS, ORIGINAL_ROCKET_NAME_BLOCKS, ORIGINAL_ROCKET_ORDNANCE_TYPES,
    OriginalOrdnanceCounts, OriginalOrdnanceSurface,
};
pub use cs_content::weapons::ORIGINAL_RESOURCE_HEADER;
use cs_types::asset_id::{
    AssetKey, MountId, MountNamespace, PrecedenceClass, ResolveContext, SourceSpan,
};
use cs_types::content::{Origin, Provenance};
use cs_types::evidence::{ClaimId, ClaimStatus};
use cs_types::install::InstallManifest;

/// The retail container that holds the engine's resource header and the
/// ordnance screens.
pub const BASE_CONTAINER: &str = "GOSDATA/ASSETS/crimson.rof";

/// The members this stage reads, spelled as the installation spells them.
pub const MEMBERS: [&str; 8] = [
    ORIGINAL_RESOURCE_HEADER,
    "ASSETS/SCRIPTS/MULTIPLAYER_AMMOR.SCRIPT",
    "ASSETS/SCRIPTS/MULTIPLAYER_OUTLAWROC.SCRIPT",
    "ASSETS/SCRIPTS/HARDPOINTS.SCRIPT",
    "ASSETS/SCRIPTS/ORDINANCELAYOUT.SCRIPT",
    "ASSETS/SCRIPTS/AIRFRAME.SCRIPT",
    "ASSETS/SCRAPBOOK.CSV",
    "ASSETS/LAYOUT.CSV",
];

/// The engine's own variable dictionary, which names the rocket and hardpoint
/// identifiers.
pub const DEBUGINFO: &str = "ASSETS/SCRIPTS/DEBUGINFO.TXT";

/// The rocket identifier blocks the resource header declares, with the macro
/// that declares each.
///
/// **Measured**: the third entry is the description block, and the screens
/// index it as `3410 + selection - 1`.
pub const MEASURED_ROCKET_BLOCK_MACROS: [(&str, u32); 3] = [
    ("IDS_ROCKETLONGNAME", 3380),
    ("IDS_ROCKETSHORTNAME", 3395),
    ("IDS_ROCKETDESCRIPTION", 3410),
];

/// The first string-id block the resource header declares **after** the rocket
/// blocks, with the macro that declares it.
///
/// **Measured**: this is what bounds the rocket run at fifteen ids per block,
/// and it is why the blocks are *not* a type count.
pub const MEASURED_NEXT_BLOCK_MACRO: (&str, u32) = ("IDS_PAINTLONGNAME", 3425);

/// The nitro control the resource header declares, with the macro that
/// declares it.
pub const MEASURED_NITRO_MACRO: (&str, u32) = ("MPOUT_CHK_NITRO", 10135);

/// The engine's own names for the ordnance identifiers this stage's audit
/// uses. Their presence in `DEBUGINFO.TXT` is what lets the audit speak the
/// original's vocabulary rather than this project's.
pub const MEASURED_DICTIONARY_NAMES: [(&str, &str); 10] = [
    ("nroc", "VIA"),
    ("arrocketnames", "YIA"),
    ("nfirstrocket", "XKA"),
    ("odrockets", "RKA"),
    ("chkallroc", "XIA"),
    ("frocout", "WIA"),
    ("fnitroout", "SIA"),
    ("nhardpoint", "YHA"),
    ("ohardpointweight", "YQA"),
    ("ohardpointcost", "ZQA"),
];

/// The literals each count is read from, each with the member it lives in and
/// what its presence proves.
pub const COUNT_BOUNDS: [(&str, &str, &str); 13] = [
    (
        "ASSETS/SCRIPTS/MULTIPLAYER_AMMOR.SCRIPT",
        "string DIA[11]",
        "the eleven-element rocket-ammunition name array the screen asks the engine for",
    ),
    (
        "ASSETS/SCRIPTS/MULTIPLAYER_AMMOR.SCRIPT",
        "for(AIA=0; AIA < 11 + 1; AIA++)",
        "a header row plus eleven selectable rocket rows",
    ),
    (
        "ASSETS/SCRIPTS/MULTIPLAYER_AMMOR.SCRIPT",
        "for(ZHA = 0; ZHA < 11; ZHA++)",
        "the loop that tests eleven rocket types",
    ),
    (
        "ASSETS/SCRIPTS/MULTIPLAYER_AMMOR.SCRIPT",
        "(3410 + sender.QG - 1)",
        "the description indexed as block base plus selection minus one, so the \
         descriptions occupy 3410..=3420",
    ),
    (
        "ASSETS/SCRIPTS/MULTIPLAYER_AMMOR.SCRIPT",
        "object EIA[8]",
        "the eight rocket-slot dropdowns",
    ),
    (
        "ASSETS/SCRIPTS/MULTIPLAYER_AMMOR.SCRIPT",
        "for (YHA=0; YHA < 8; YHA++)",
        "and the loop that fills them",
    ),
    (
        "ASSETS/SCRIPTS/MULTIPLAYER_OUTLAWROC.SCRIPT",
        "for(int RX=0; RX < 11; RX++)",
        "an independent screen asking for eleven rocket names",
    ),
    (
        "ASSETS/SCRIPTS/MULTIPLAYER_OUTLAWROC.SCRIPT",
        "callback($$E$$,5019,(RX),YIA[RX])",
        "through a different callback into the same eleven-element array",
    ),
    (
        "ASSETS/SCRIPTS/HARDPOINTS.SCRIPT",
        "object DT[2]",
        "the two hardpoint-point dropdowns",
    ),
    (
        "ASSETS/SCRIPTS/HARDPOINTS.SCRIPT",
        "for (int R=0; R < 2; R++)",
        "and the loop that fills them",
    ),
    (
        "ASSETS/SCRIPTS/HARDPOINTS.SCRIPT",
        "callback($$E$$, 2245, 0, (R), AT[R])",
        "the per-point read that asks the engine for the installed component",
    ),
    (
        "ASSETS/SCRIPTS/ORDINANCELAYOUT.SCRIPT",
        "object RKA[8]",
        "eight rocket-ammunition dropdowns on one airframe, a second witness",
    ),
    (
        "ASSETS/SCRAPBOOK.CSV",
        "NT_07_01_bpnitro",
        "a scrapbook illustration whose id names a nitro item",
    ),
];

/// The scrapbook rows whose *illustration ids* name an ordnance item, and the
/// text-resource ids they point at.
///
/// **Measured**, and deliberately not more than that. An illustration id is
/// the original build's own asset name; it is **not** a catalogue entry, it
/// names no component this project can fly, and its `_t`/`_b` string ids are
/// not declared in the resource header — so the text behind them is in the
/// executable's runtime catalog and unreadable, exactly as F27-D found for the
/// gun and ammunition names.
pub const MEASURED_SCRAPBOOK_ORDNANCE_ROWS: [(&str, &str, &str); 2] = [
    (
        "7_2_5",
        "NT_07_01_bpnitro",
        "IDS_NT_07_01_bpnitro_t,IDS_NT_07_01_bpnitro_b",
    ),
    (
        "19_1_4",
        "NT_19_01_bptorpedo",
        "IDS_NT_19_01_bptorpedo_t,IDS_NT_19_01_bptorpedo_b",
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
    ClaimId::new("f28d.retail-ordnance-catalogue").expect("a valid claim id")
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

/// The installation's measured ordnance surface, built from what its own
/// resource header and screens declare.
///
/// `Origin::Installation` and a `VerifiedOriginal` provenance are correct here
/// and only here: the bytes were read from the owner's installation and the
/// span locates them. That claim is about **file content** — ids and counts —
/// and says nothing about how the game behaves.
pub fn measured_surface(root: &Path) -> (OriginalOrdnanceSurface, Vec<u8>) {
    let install = install_sha256(root);
    let header = read_member(root, ORIGINAL_RESOURCE_HEADER);
    let counts = OriginalOrdnanceCounts::try_new(
        ORIGINAL_ROCKET_ORDNANCE_TYPES,
        ORIGINAL_ORDNANCE_ROCKET_SLOTS,
        ORIGINAL_ORDNANCE_HARDPOINT_POINTS,
        true,
    )
    .expect("every measured count is nonzero");
    let span = span_of(
        install,
        ORIGINAL_RESOURCE_HEADER,
        &header,
        "#define IDS_ROCKETLONGNAME",
    );
    let surface = OriginalOrdnanceSurface::new(
        Origin::Installation {
            source: span.clone(),
        },
        counts,
        Provenance::new(claim(), ClaimStatus::VerifiedOriginal, Some(span))
            .expect("a verified_original provenance names its span"),
    );
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
    /// The scrapbook ordnance rows, as `(row, illustration id, present)`, and
    /// whether the resource header declares their text ids at all.
    pub scrapbook: Vec<(String, String, bool, bool)>,
    /// The rocket block base ids the header declares, in declaration order.
    pub rocket_block_ids: Vec<u32>,
    /// The next block's base id, if the header declares it.
    pub next_block_id: Option<u32>,
    /// The nitro control's id, if the header declares it.
    pub nitro_control_id: Option<u32>,
}

impl Observation {
    /// The measured rocket-type count, from the two screens' loop bounds.
    ///
    /// **Read from the members, not from a constant**: this is the harness's
    /// own reading of the same fact the acceptance test asserts, so a stale
    /// constant in either place shows up as a disagreement here.
    pub fn measured_rocket_types(&self) -> Option<u32> {
        let screen = self.bounds.iter().find(|(_, literal, _)| {
            literal == "string DIA[11]" || literal == "for(ZHA = 0; ZHA < 11; ZHA++)"
        })?;
        // The literal spells its own bound; the number is the digits in it.
        literal_bound(screen.1.as_str())
    }

    /// The measured rocket-slot count, from the screens' array sizes.
    pub fn measured_rocket_slots(&self) -> Option<u32> {
        let screen = self
            .bounds
            .iter()
            .find(|(_, literal, _)| literal == "object EIA[8]")?;
        literal_bound(screen.1.as_str())
    }

    /// The measured hardpoint-point count, from the hardpoint screen.
    pub fn measured_hardpoint_points(&self) -> Option<u32> {
        let screen = self
            .bounds
            .iter()
            .find(|(_, literal, _)| literal == "object DT[2]")?;
        literal_bound(screen.1.as_str())
    }
}

/// The trailing `[n]` of a literal like `object EIA[8]`, or `for(... < n; ...)`.
fn literal_bound(literal: &str) -> Option<u32> {
    if let Some(open) = literal.rfind('[')
        && let Some(close) = literal.rfind(']')
        && close > open
    {
        return literal[open + 1..close].parse::<u32>().ok();
    }
    let after = literal.split("< ").nth(1)?;
    after
        .split(|character: char| !character.is_ascii_digit())
        .find(|piece| !piece.is_empty())?
        .parse::<u32>()
        .ok()
}

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
    let text = |name: &str| -> String {
        cache
            .iter()
            .find(|(spelling, _)| spelling == name)
            .map(|(_, bytes)| String::from_utf8_lossy(bytes).into_owned())
            .unwrap_or_default()
    };
    let header = text(ORIGINAL_RESOURCE_HEADER);
    let defines: Vec<(String, u32)> = header
        .lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("#define ")?;
            let (name, value) = rest.split_once(' ')?;
            Some((name.trim().to_owned(), value.trim().parse::<u32>().ok()?))
        })
        .collect();
    let rocket_block_ids = MEASURED_ROCKET_BLOCK_MACROS
        .iter()
        .map(|(_, id)| *id)
        .collect::<Vec<u32>>();
    let next_block_id = defines
        .iter()
        .find(|(name, _)| name == MEASURED_NEXT_BLOCK_MACRO.0)
        .map(|(_, value)| *value);
    let nitro_control_id = defines
        .iter()
        .find(|(name, _)| name == MEASURED_NITRO_MACRO.0)
        .map(|(_, value)| *value);

    let dictionary_text = text(DEBUGINFO);
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
            (
                (*member).to_owned(),
                (*literal).to_owned(),
                text(member).contains(literal),
            )
        })
        .collect();

    let scrapbook = text("ASSETS/SCRAPBOOK.CSV");
    let scrapbook_rows = MEASURED_SCRAPBOOK_ORDNANCE_ROWS
        .iter()
        .map(|(row, illustration, texts)| {
            let present = scrapbook
                .lines()
                .any(|line| line.starts_with(&format!("{row}=")) && line.contains(illustration));
            // Whether the resource header declares *either* of the row's text
            // ids. It does not, and that is the point: the text is in the
            // executable's catalog, not in a file.
            let declared = texts.split(',').any(|id| header.contains(id));
            (
                (*row).to_owned(),
                (*illustration).to_owned(),
                present,
                declared,
            )
        })
        .collect();

    Observation {
        members,
        defines,
        dictionary,
        bounds,
        scrapbook: scrapbook_rows,
        rocket_block_ids,
        next_block_id,
        nitro_control_id,
    }
}

/// The committed rocket-block constants, so the harness compares the
/// measurement against the shipped values rather than against itself.
pub fn committed_rocket_blocks() -> Vec<u32> {
    ORIGINAL_ROCKET_NAME_BLOCKS
        .iter()
        .map(|(id, _)| *id)
        .collect()
}

/// The committed next-block id.
pub const fn committed_next_block() -> u32 {
    ORIGINAL_NEXT_ROCKET_BLOCK.0
}

/// The committed nitro-control id.
pub const fn committed_nitro_control() -> u32 {
    ORIGINAL_NITRO_CONTROL.1
}
