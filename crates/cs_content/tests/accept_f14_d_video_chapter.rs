//! Acceptance scenario F14-D.VIDEO-CHAPTER: the chapter-video binding of the
//! `video` collection of the retail baseline inventory (task #1352, stage
//! `### F14-D` of `specs/F14-canonical-content-catalog-and-dependency-closure.md`).
//!
//! The installation ships `chap0.mpg`…`chap5.mpg` under
//! `GOSDATA/ASSETS/GRAPHICS/MPG/`, but no record of either video source names
//! `chap1`…`chap5`: `ASSETS/SCRIPTS/CAMPAIGNINTRO.SCRIPT` *constructs* the
//! file name as `GC.BL = "chap" conv$(FC) ".mpg"` from `FC`, a variable
//! filled by `callback($$E$$, 2151, FC)`. `ASSETS/SCRIPTS/CTL.SCRIPT`
//! declares the handle a movie (`movie CL`), resolves the constructed name
//! as `CL = "assets\\graphics\\mpg\\" BL` and plays it (`loop CL`,
//! `getstatus(CL)`), and `ASSETS/SCRIPTS/PASSENGERCABIN.SCRIPT` calls the
//! same callback and `script_run`s the intro member only `if (FC != 0)`.
//!
//! **What this stage adds.** A byte-for-byte measurement of that
//! three-member construction chain through the production ROF mount and the
//! F38-B host-call scanner: each member must read, scan as a UI script
//! program, show its measured `callback` sites and contain its measured
//! byte fragments. Every unverifiable member is counted under
//! `chapter_source_unreadable` and named in the diagnostic. An inventoried
//! `.mpg` file no record of either source reaches whose name additionally
//! spells the construction's product shape — `chap`, one or more digits,
//! `.mpg` — is counted under `chapter_name_unbound` in addition to
//! `unnamed_video_file`: the machine-readable record that the binding was
//! not read.
//!
//! **What this stage refuses.** No row is minted for `chap1`…`chap5`: the
//! construction states no file name until `FC` is bound, and which values
//! `FC` takes is the return domain of native callback 2151 — engine state,
//! not an installation byte (F38-B already records native-callback meanings
//! as unread, and no member, record or flag names the domain). A file-name
//! match is a name shape, never a binding, so the files stay gaps and never
//! become rows.
//!
//! The non-retail tests write **synthetic installation trees** into
//! temporary directories — including a two-level ROF container with an
//! `ASSETS/SCRIPTS/` directory holding authored script members — that the
//! production `retail_baseline` really reads. Removing the chapter
//! measurement fails them.
//!
//! The retail test (`#[ignore = "requires CS_GAME_DIR"]`) reads the owner's
//! original installation, re-measures the three members through the
//! production readers and pins the five unbound chapter files. Run it with
//! `--include-ignored`; without `CS_GAME_DIR` it fails loudly.
//!
//! The authored member spellings reproduce only the measured bytes the
//! chain is verified against. No original content is committed.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use cs_content::catalog::baseline::{
    Baseline, CollectionStatus, FMV_ARCHIVE, FMV_MEMBER, VIDEO_LAYOUT_CONTAINER,
    baseline_report_json, install_file_key, retail_baseline,
};
use cs_formats::{DIRECTORY_HEADER_BYTES, FLAG_DIRECTORY, RECORD_BYTES};
use cs_types::content::{CatalogElement, ContentId, ContentKind};

/// A disposable installation tree, removed on drop.
struct TempInstall(PathBuf);

impl TempInstall {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f14-d-video-chapter-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("the fixture directory is created");
        Self(root)
    }

    fn write(&self, relative: &str, bytes: &[u8]) {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().expect("a parent directory"))
            .expect("the fixture directory is created");
        fs::write(&path, bytes).expect("the fixture file is written");
    }
}

impl Drop for TempInstall {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// ------------------------------------------------------------- .zrd -------

/// A `.zrd` integer node: tag `1` and the value.
fn zrd_int(value: u32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&value.to_le_bytes());
    bytes
}

/// A `.zrd` text node: tag `3`, the byte length and the bytes.
fn zrd_text(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + text.len());
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

/// A `.zrd` list node: tag `4`, then **`children.len() + 1`** as the count,
/// then the children (the measured grammar: the writer stores one more than
/// the child count).
fn zrd_list(children: Vec<Vec<u8>>) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + children.iter().map(Vec::len).sum::<usize>());
    bytes.extend_from_slice(&4_u32.to_le_bytes());
    bytes.extend_from_slice(&((children.len() as u32) + 1).to_le_bytes());
    for child in children {
        bytes.extend_from_slice(&child);
    }
    bytes
}

// ------------------------------------------------------ reader archives ---

/// A version-one reader archive holding `members` in order: the member data,
/// then one 148-byte index entry each (u32 start, u32 length, a 64-byte
/// NUL-padded name and 76 bytes), then the u32 version `1` and u32 count.
fn reader_archive(members: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut entries = Vec::with_capacity(members.len());
    for (name, member) in members {
        let start = bytes.len() as u32;
        bytes.extend_from_slice(member);
        entries.push((start, member.len() as u32, *name));
    }
    for (start, length, name) in &entries {
        bytes.extend_from_slice(&start.to_le_bytes());
        bytes.extend_from_slice(&length.to_le_bytes());
        let name = name.as_bytes();
        assert!(name.len() < 64, "a fixture member name fits its field");
        let mut field = [0_u8; 64];
        field[..name.len()].copy_from_slice(name);
        bytes.extend_from_slice(&field);
        bytes.extend_from_slice(&[0_u8; 76]);
    }
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&(members.len() as u32).to_le_bytes());
    bytes
}

/// One authored `PLAYAVI` action: the tag and the file it names.
fn playavi(file: &str) -> Vec<u8> {
    zrd_list(vec![zrd_text("PLAYAVI"), zrd_text(file)])
}

/// An authored `fmv.zrd`: one `[sequence, actions]` pair per sequence, the
/// measured key/value shape.
fn fmv_document(sequences: &[(&str, Vec<Vec<u8>>)]) -> Vec<u8> {
    let mut children = vec![zrd_text("IMAGE_PATH"), zrd_list(vec![zrd_text("video")])];
    for (name, actions) in sequences {
        children.push(zrd_text(name));
        children.push(zrd_list(actions.clone()));
    }
    zrd_list(children)
}

// -------------------------------------------------------------- ROF -------

/// One authored member of a synthetic ROF container.
struct Entry<'a> {
    name: &'a str,
    /// The stored extent: verbatim bytes for a plain member.
    stored: Vec<u8>,
    /// The record's `raw_length` word: the decoded count.
    decoded_len: u32,
    flags: u32,
}

impl<'a> Entry<'a> {
    fn plain(name: &'a str, bytes: &'a [u8]) -> Self {
        Self {
            name,
            stored: bytes.to_vec(),
            decoded_len: bytes.len() as u32,
            flags: 0,
        }
    }
}

fn rof_names(names: &[&str]) -> Vec<u8> {
    let mut table = Vec::new();
    for name in names {
        table.extend_from_slice(name.as_bytes());
        table.push(0);
    }
    table
}

/// One directory block: header, 24-byte records, name table.
fn rof_block(records: &[[u32; 6]], names: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(records.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(names.len() as u32).to_le_bytes());
    for record in records {
        for word in record {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
    }
    bytes.extend_from_slice(names);
    bytes
}

/// `[root][ASSETS][SCRIPTS][payloads]` — the retail archive's two-level
/// shape: `ASSETS/` holds `layout` and the `SCRIPTS/` directory, which holds
/// the authored script members.
fn rof_container_scripts(layout: &Entry<'_>, scripts: &[Entry<'_>]) -> Vec<u8> {
    let root_names = rof_names(&["ASSETS"]);
    let root_len = DIRECTORY_HEADER_BYTES + RECORD_BYTES + root_names.len();
    let assets_names = rof_names(&[layout.name, "SCRIPTS"]);
    let assets_len = DIRECTORY_HEADER_BYTES + 2 * RECORD_BYTES + assets_names.len();
    let script_names: Vec<&str> = scripts.iter().map(|entry| entry.name).collect();
    let scripts_names = rof_names(&script_names);
    let scripts_len = DIRECTORY_HEADER_BYTES + scripts.len() * RECORD_BYTES + scripts_names.len();

    let scripts_off = (root_len + assets_len) as u32;
    let root = rof_block(
        &[[
            root_len as u32,
            0,
            0,
            FLAG_DIRECTORY,
            "ASSETS".len() as u32 + 1,
            1,
        ]],
        &root_names,
    );

    let mut cursor = scripts_off + scripts_len as u32;
    let assets_records = vec![
        [
            cursor,
            layout.decoded_len,
            layout.stored.len() as u32,
            layout.flags,
            layout.name.len() as u32 + 1,
            10,
        ],
        [
            scripts_off,
            0,
            0,
            FLAG_DIRECTORY,
            "SCRIPTS".len() as u32 + 1,
            11,
        ],
    ];
    cursor += layout.stored.len() as u32;

    let mut script_records = Vec::with_capacity(scripts.len());
    for (index, entry) in scripts.iter().enumerate() {
        script_records.push([
            cursor,
            entry.decoded_len,
            entry.stored.len() as u32,
            entry.flags,
            entry.name.len() as u32 + 1,
            10 + index as u32,
        ]);
        cursor += entry.stored.len() as u32;
    }

    let mut bytes = root;
    bytes.extend_from_slice(&rof_block(&assets_records, &assets_names));
    bytes.extend_from_slice(&rof_block(&script_records, &scripts_names));
    bytes.extend_from_slice(&layout.stored);
    for entry in scripts {
        bytes.extend_from_slice(&entry.stored);
    }
    assert_eq!(bytes.len(), cursor as usize, "every payload placed once");
    bytes
}

// ------------------------------------------------- the authored members ---

/// The authored intro member: the measured site and construction spelling
/// the retail member shows, inside a program that scans.
const INTRO_MEMBER: &[u8] = b"main\r\n{\r\ngui_create\r\n{\r\nint FC\r\ncallback($$E$$, 2151, \
     FC)\r\nobject GC = @ctl@AL\r\n}\r\ngui_init\r\n{\r\nGC.BL = \"chap\" conv$(FC) \
     \".mpg\"\r\ninitialize(GC)\r\n}\r\n}\r\n";

/// The authored control member: the movie declaration, the
/// `assets\graphics\mpg\` resolution and the play/poll the retail member
/// shows.
const CONTROL_MEMBER: &[u8] = b"main\r\n{\r\ngui_init\r\n{\r\nmovie CL\r\nCL = \
     \"assets\\\\graphics\\\\mpg\\\\\" BL\r\nloop CL\r\nif (status_stopped == \
     getstatus(CL))\r\n{\r\nmail (1, this)\r\n}\r\n}\r\n}\r\n";

/// The authored trigger member: the same callback site and the gated
/// `script_run` into the intro member.
const TRIGGER_MEMBER: &[u8] = b"main\r\n{\r\ngui_init\r\n{\r\nint FC\r\ncallback($$E$$, 2151, \
     FC)\r\nif (FC != 0)\r\n{\r\nscript_run \"assets\\\\scripts\\\\\" \
     \"campaignintro.script\", 0x1999\r\n}\r\n}\r\n}\r\n";

/// The three chapter-construction members, spelled as the retail chain
/// verifies them.
fn chapter_scripts() -> Vec<Entry<'static>> {
    vec![
        Entry::plain("CAMPAIGNINTRO.SCRIPT", INTRO_MEMBER),
        Entry::plain("CTL.SCRIPT", CONTROL_MEMBER),
        Entry::plain("PASSENGERCABIN.SCRIPT", TRIGGER_MEMBER),
    ]
}

// -------------------------------------------------------------- trees -----

/// The per-mission members a scenario-shaped reader must list, so the
/// campaign walk finds the mission directory.
const MISSION_MEMBERS: [&str; 3] = ["map.zrd", "aiv.zrd", "objectives.zrd"];

/// A minimal installation: one campaign mission (so the shared campaign walk
/// finds a layout), the shared reader archive `FMV_ARCHIVE` holding
/// `fmv.zrd`, an optional `GOSDATA/ASSETS/crimson.rof` holding a layout
/// member and a `SCRIPTS/` directory of authored members, and any loose
/// files the caller names.
fn tree(
    label: &str,
    fmv: Option<Vec<u8>>,
    rof: Option<(&Entry<'_>, Vec<Entry<'_>>)>,
    loose: &[(&str, &[u8])],
) -> TempInstall {
    let temp = TempInstall::new(label);
    temp.write(
        "ZBD/C1C/M01/zrdr.zbd",
        &reader_archive(&[
            ("net.zrd", zrd_int(1)),
            (MISSION_MEMBERS[0], zrd_int(1)),
            (MISSION_MEMBERS[1], zrd_int(1)),
            (MISSION_MEMBERS[2], zrd_int(1)),
        ]),
    );
    temp.write("ZBD/C1C/M01/mis_anim.zbd", b"mission animation bytes");
    if let Some(document) = fmv {
        temp.write(FMV_ARCHIVE, &reader_archive(&[(FMV_MEMBER, document)]));
    }
    if let Some((layout, scripts)) = rof {
        temp.write(
            VIDEO_LAYOUT_CONTAINER,
            &rof_container_scripts(layout, &scripts),
        );
    }
    for (spelling, bytes) in loose {
        temp.write(spelling, bytes);
    }
    temp
}

/// An authored layout member: one movie record naming `Final.MPG`.
const LAYOUT_MEMBER: &[u8] = b";Multimedia\n\
    [@FinalCinema@]\n\
    CF_MOVIE=M,Final.MPG,0,0,0,250,250,1,0\n";

// -------------------------------------------------------------- mapping ---

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("test content id is valid")
}

fn status_of(baseline: &Baseline, kind: ContentKind) -> &CollectionStatus {
    baseline
        .collection_status
        .iter()
        .find(|status| status.kind == kind)
        .unwrap_or_else(|| panic!("the {kind} collection reports its status"))
}

fn rows_of(baseline: &Baseline, kind: ContentKind) -> Vec<&CatalogElement> {
    baseline
        .catalog
        .elements()
        .filter(|element| element.kind == kind)
        .collect()
}

/// The video row whose resolved file is `spelling`, when there is one.
fn video_row_for<'a>(baseline: &'a Baseline, spelling: &str) -> Option<&'a CatalogElement> {
    rows_of(baseline, ContentKind::Video)
        .into_iter()
        .find(|row| row.id == cid(ContentKind::Video, &install_file_key(spelling)))
}

// ------------------------------------------------------------ the tests ---

/// The whole measured chain verifies — and **still** no row is minted for a
/// constructed name: `chapter_name_unbound` counts the files whose names
/// spell `"chap" conv$(FC) ".mpg"`'s product shape, on top of
/// `unnamed_video_file`, because the `FC` binding was not read.
#[test]
fn accept_f14_d_video_chapter_constructed_names_stay_unbound() {
    let layout = Entry::plain("LAYOUT.CSV", LAYOUT_MEMBER);
    let temp = tree(
        "unbound",
        Some(fmv_document(&[("CHAP0", vec![playavi("chap0.mpg")])])),
        Some((&layout, chapter_scripts())),
        &[
            (
                "GOSDATA/ASSETS/GRAPHICS/MPG/chap0.mpg",
                b"chapter zero video",
            ),
            ("GOSDATA/ASSETS/GRAPHICS/MPG/final.mpg", b"final video"),
            // Two files spelling the construction's product shape: the
            // measured chain could produce their names for some `FC`, but
            // no installation byte states which values `FC` takes.
            (
                "GOSDATA/ASSETS/GRAPHICS/MPG/chap1.mpg",
                b"chapter one video",
            ),
            (
                "GOSDATA/ASSETS/GRAPHICS/MPG/chap2.mpg",
                b"chapter two video",
            ),
            // A real `.mpg` whose name does not spell the chapter shape.
            ("GOSDATA/ASSETS/GRAPHICS/MPG/other.mpg", b"other video"),
        ],
    );
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    let rows = rows_of(&baseline, ContentKind::Video);
    assert_eq!(rows.len(), 2, "chap0 via PLAYAVI, final via layout");
    for spelling in [
        "GOSDATA/ASSETS/GRAPHICS/MPG/chap1.mpg",
        "GOSDATA/ASSETS/GRAPHICS/MPG/chap2.mpg",
        "GOSDATA/ASSETS/GRAPHICS/MPG/other.mpg",
    ] {
        assert!(
            video_row_for(&baseline, spelling).is_none(),
            "{spelling}: a file name that only the unbound construction could \
             reach never becomes a row"
        );
    }

    let status = status_of(&baseline, ContentKind::Video);
    assert_eq!(status.rows, 2);
    assert_eq!(
        status.gaps,
        BTreeMap::from([
            ("playavi_name_not_text", 0),
            ("movie_record_without_mpg_name", 0),
            ("named_file_absent", 0),
            ("ambiguous_file_name", 0),
            ("duplicate_video_name", 0),
            // `chap1`, `chap2` and `other`: no record of either source
            // reaches them.
            ("unnamed_video_file", 3),
            // `chap1` and `chap2` additionally spell the construction's
            // product shape — the explicit record that the binding was not
            // read, on top of the unaccounted-for count.
            ("chapter_name_unbound", 2),
            // All three chain members verified.
            ("chapter_source_unreadable", 0),
            ("unreadable_video_source", 0),
        ])
    );
    assert_eq!(status.diagnostic, None);

    // The denominator did not move: a video is not launchable content.
    assert_eq!(baseline.roots, vec![cid(ContentKind::Mission, "ch1-m01")]);
    assert_eq!(baseline.coverage.roots, 1);
    assert_eq!(baseline.catalog.launchable_count(), 1);
    assert_eq!(baseline.coverage.unreachable_by_kind.get("video"), Some(&2));

    let report = baseline_report_json(&baseline);
    assert!(report.contains("\"chapter_name_unbound\":2"), "{report}");
    assert!(
        report.contains("\"chapter_source_unreadable\":0"),
        "{report}"
    );
}

/// A chain member that does not show its measured bytes is counted under
/// `chapter_source_unreadable` and named in the diagnostic — and the
/// `chapter_name_unbound` record still counts the name-shape files, resting
/// on the file-name shape alone, never on a guess.
#[test]
fn accept_f14_d_video_chapter_unverified_members_are_named() {
    // The intro member keeps the callback site but spells a different
    // construction: the measured fragment is absent.
    let broken_intro: &[u8] = b"main\r\n{\r\ngui_create\r\n{\r\nint FC\r\ncallback($$E$$, 2151, \
         FC)\r\n}\r\ngui_init\r\n{\r\nGC.BL = \"clip\" conv$(FC) \".mpg\"\r\n}\r\n}\r\n";
    let layout = Entry::plain("LAYOUT.CSV", LAYOUT_MEMBER);
    let scripts = vec![
        Entry::plain("CAMPAIGNINTRO.SCRIPT", broken_intro),
        Entry::plain("CTL.SCRIPT", CONTROL_MEMBER),
        Entry::plain("PASSENGERCABIN.SCRIPT", TRIGGER_MEMBER),
    ];
    let temp = tree(
        "broken",
        Some(fmv_document(&[("CHAP0", vec![playavi("chap0.mpg")])])),
        Some((&layout, scripts)),
        &[
            (
                "GOSDATA/ASSETS/GRAPHICS/MPG/chap0.mpg",
                b"chapter zero video",
            ),
            ("GOSDATA/ASSETS/GRAPHICS/MPG/final.mpg", b"final video"),
            (
                "GOSDATA/ASSETS/GRAPHICS/MPG/chap1.mpg",
                b"chapter one video",
            ),
        ],
    );
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    let status = status_of(&baseline, ContentKind::Video);
    assert_eq!(status.rows, 2);
    assert_eq!(
        status.gaps,
        BTreeMap::from([
            ("playavi_name_not_text", 0),
            ("movie_record_without_mpg_name", 0),
            ("named_file_absent", 0),
            ("ambiguous_file_name", 0),
            ("duplicate_video_name", 0),
            ("unnamed_video_file", 1),
            ("chapter_name_unbound", 1),
            // The intro member alone fails its measured fragment.
            ("chapter_source_unreadable", 1),
            ("unreadable_video_source", 0),
        ])
    );
    let diagnostic = status
        .diagnostic
        .as_deref()
        .expect("the unverifiable member is named");
    assert!(
        diagnostic.contains("ASSETS/SCRIPTS/CAMPAIGNINTRO.SCRIPT"),
        "{diagnostic}"
    );
    assert!(diagnostic.contains("measured fragment"), "{diagnostic}");
}

/// A container that holds none of the chain members — or no container at
/// all — leaves every member unverifiable, counted and named; the
/// `PLAYAVI` row still builds beside the gap.
#[test]
fn accept_f14_d_video_chapter_absent_chain_is_counted() {
    let layout = Entry::plain("LAYOUT.CSV", LAYOUT_MEMBER);
    // The container holds only the layout member: the `SCRIPTS/` directory
    // is absent entirely.
    let temp = tree(
        "no-scripts",
        Some(fmv_document(&[("CHAP0", vec![playavi("chap0.mpg")])])),
        Some((&layout, Vec::new())),
        &[
            (
                "GOSDATA/ASSETS/GRAPHICS/MPG/chap0.mpg",
                b"chapter zero video",
            ),
            ("GOSDATA/ASSETS/GRAPHICS/MPG/final.mpg", b"final video"),
            (
                "GOSDATA/ASSETS/GRAPHICS/MPG/chap4.mpg",
                b"chapter four video",
            ),
        ],
    );
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    let status = status_of(&baseline, ContentKind::Video);
    assert_eq!(status.rows, 2);
    assert_eq!(status.gaps["chapter_name_unbound"], 1);
    assert_eq!(status.gaps["chapter_source_unreadable"], 3);
    let diagnostic = status
        .diagnostic
        .as_deref()
        .expect("the unverifiable members are named");
    for member in [
        "ASSETS/SCRIPTS/CAMPAIGNINTRO.SCRIPT",
        "ASSETS/SCRIPTS/CTL.SCRIPT",
        "ASSETS/SCRIPTS/PASSENGERCABIN.SCRIPT",
    ] {
        assert!(diagnostic.contains(member), "{diagnostic}");
    }
}

/// The name-shape predicate is the measured `chap<digits>.mpg` only: case is
/// folded like every other name comparison, and a missing, non-digit or
/// negative middle falls back to plain `unnamed_video_file`.
#[test]
fn accept_f14_d_video_chapter_name_shape_is_measured() {
    let layout = Entry::plain("LAYOUT.CSV", LAYOUT_MEMBER);
    let temp = tree(
        "shape",
        Some(fmv_document(&[("CHAP0", vec![playavi("chap0.mpg")])])),
        Some((&layout, chapter_scripts())),
        &[
            (
                "GOSDATA/ASSETS/GRAPHICS/MPG/chap0.mpg",
                b"chapter zero video",
            ),
            ("GOSDATA/ASSETS/GRAPHICS/MPG/final.mpg", b"final video"),
            // Matches: `chap` + digits + `.mpg`, case-folded.
            ("GOSDATA/ASSETS/GRAPHICS/MPG/chap3.mpg", b"chapter video"),
            ("GOSDATA/ASSETS/GRAPHICS/MPG/CHAP4.MPG", b"chapter video"),
            // No match: empty, non-digit, signed or prefixed middles, and a
            // non-`.mpg` suffix is not counted a video file at all.
            ("GOSDATA/ASSETS/GRAPHICS/MPG/chap.mpg", b"no digits"),
            (
                "GOSDATA/ASSETS/GRAPHICS/MPG/chapx1.mpg",
                b"non-digit middle",
            ),
            ("GOSDATA/ASSETS/GRAPHICS/MPG/xchap5.mpg", b"prefixed"),
            ("GOSDATA/ASSETS/GRAPHICS/MPG/chap-1.mpg", b"signed middle"),
            ("GOSDATA/ASSETS/GRAPHICS/MPG/chap6.mpeg", b"not an mpg"),
        ],
    );
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    let status = status_of(&baseline, ContentKind::Video);
    assert_eq!(status.rows, 2);
    assert_eq!(
        status.gaps,
        BTreeMap::from([
            ("playavi_name_not_text", 0),
            ("movie_record_without_mpg_name", 0),
            ("named_file_absent", 0),
            ("ambiguous_file_name", 0),
            ("duplicate_video_name", 0),
            // `chap3`, `CHAP4`, `chap`, `chapx1`, `xchap5`, `chap-1`: six
            // `.mpg` files no record reaches.
            ("unnamed_video_file", 6),
            // Only `chap3.mpg` and `CHAP4.MPG` spell the construction's
            // product shape.
            ("chapter_name_unbound", 2),
            ("chapter_source_unreadable", 0),
            ("unreadable_video_source", 0),
        ])
    );
    assert_eq!(status.diagnostic, None);
}

// --------------------------------------------------------------- retail ---

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

/// Read `member` of the retail shared archive through the production ROF
/// mount, returning its decoded bytes.
fn retail_member(dir: &std::path::Path, install_sha256: &str, member: &str) -> Vec<u8> {
    let mut builder = cs_assets::vfs::SessionBuilder::new(cs_types::asset_id::ResolveContext::new(
        cs_types::evidence::ContentHash::from_hex(install_sha256)
            .expect("the reported fingerprint is a content hash"),
    ));
    let mount = cs_assets::vfs::MountBuilder::new(
        cs_types::asset_id::MountId::new("rof-video-chapter-check").expect("valid mount id"),
        cs_types::asset_id::MountNamespace::new(cs_assets::vfs::INSTALL_NAMESPACE)
            .expect("valid namespace"),
        cs_types::asset_id::PrecedenceClass::Shared,
        VIDEO_LAYOUT_CONTAINER,
    )
    .retail();
    let source =
        cs_assets::rof::mount_rof_into(&mut builder, mount, &dir.join(VIDEO_LAYOUT_CONTAINER))
            .expect("the retail shared archive mounts");
    let key =
        cs_types::asset_id::AssetKey::from_spelling(source.namespace().as_str(), member, "default")
            .expect("the member key is valid");
    source
        .read(&key)
        .unwrap_or_else(|error| panic!("the member {member} reads: {error}"))
        .data
}

/// The retail half: the three-member chain is fully verified
/// (`chapter_source_unreadable` 0), the five chapter files the sources do
/// not name are counted under both `unnamed_video_file` and
/// `chapter_name_unbound`, and no row is minted for them — the `FC` binding
/// behind native callback 2151 is not readable from the installation's
/// bytes, exactly as
/// `docs/findings/2026-10-13-f14-d-video-chapter-binding.md` records.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f14_d_video_chapter_retail_binding_is_unread() {
    let dir = game_dir();
    let baseline = retail_baseline(&dir).expect("the original installation reads");

    // The collection's accounting: five rows from the two naming sources,
    // and the five constructed-name files counted under both codes — the
    // same files, two facts — with every chain member verified.
    let status = status_of(&baseline, ContentKind::Video);
    assert_eq!(status.rows, 5);
    assert_eq!(status.gaps["unnamed_video_file"], 5);
    assert_eq!(status.gaps["chapter_name_unbound"], 5);
    assert_eq!(status.gaps["chapter_source_unreadable"], 0);
    assert_eq!(status.diagnostic, None);

    // The five files are inventoried and hold no video row. `chap6.mpg` is
    // measured absent: the decrypted engine's string table spells it, but
    // the installation ships `chap0`…`chap5` only — the anomaly the
    // findings note records, not a file to bind.
    for number in 1..=5 {
        let spelling = format!("GOSDATA/ASSETS/GRAPHICS/MPG/chap{number}.mpg");
        assert!(
            rows_of(&baseline, ContentKind::InstallFile)
                .iter()
                .any(|row| row.display_name.as_deref() == Some(spelling.as_str())),
            "{spelling} is an inventoried file"
        );
        assert!(
            video_row_for(&baseline, &spelling).is_none(),
            "{spelling}: the unbound construction never minted a row"
        );
    }
    assert!(
        !rows_of(&baseline, ContentKind::InstallFile)
            .iter()
            .any(|row| {
                row.display_name.as_deref().is_some_and(|name| {
                    name.eq_ignore_ascii_case("GOSDATA/ASSETS/GRAPHICS/MPG/chap6.mpg")
                })
            }),
        "no chap6.mpg ships in this installation; if one ever appears the \
         measurement above and the findings note must be re-measured"
    );

    // The measurement itself, re-read through the production readers: each
    // member scans and shows its measured sites and fragments.
    use cs_formats::script_raw::{DispatchForm, UiScriptLimits, scan_ui_program};

    let intro = retail_member(
        &dir,
        &baseline.install_sha256,
        "ASSETS/SCRIPTS/CAMPAIGNINTRO.SCRIPT",
    );
    let intro_scan = scan_ui_program(
        "ASSETS/SCRIPTS/CAMPAIGNINTRO.SCRIPT",
        &intro,
        UiScriptLimits::default(),
    )
    .expect("the intro member scans");
    let intro_sites: Vec<_> = intro_scan
        .sites
        .iter()
        .filter(|site| site.form == DispatchForm::Callback && site.native_id == Some(2151))
        .collect();
    assert_eq!(intro_sites.len(), 1, "the intro member's one 2151 site");
    let span = &intro_sites[0].span;
    assert_eq!(
        &intro[span.offset as usize..span.end() as usize],
        b"callback($$E$$, 2151, FC)",
        "the site's own bytes spell the measured call"
    );
    assert!(
        intro
            .windows(b"\"chap\" conv$(FC) \".mpg\"".len())
            .any(|window| window == b"\"chap\" conv$(FC) \".mpg\""),
        "the intro member spells the measured construction"
    );

    let control = retail_member(&dir, &baseline.install_sha256, "ASSETS/SCRIPTS/CTL.SCRIPT");
    scan_ui_program(
        "ASSETS/SCRIPTS/CTL.SCRIPT",
        &control,
        UiScriptLimits::default(),
    )
    .expect("the control member scans");
    for fragment in [
        &b"movie CL"[..],
        &b"\"assets\\\\graphics\\\\mpg\\\\\" BL"[..],
        &b"loop CL"[..],
        &b"getstatus(CL)"[..],
    ] {
        assert!(
            control
                .windows(fragment.len())
                .any(|window| window == fragment),
            "the control member spells {}",
            String::from_utf8_lossy(fragment)
        );
    }

    let trigger = retail_member(
        &dir,
        &baseline.install_sha256,
        "ASSETS/SCRIPTS/PASSENGERCABIN.SCRIPT",
    );
    let trigger_scan = scan_ui_program(
        "ASSETS/SCRIPTS/PASSENGERCABIN.SCRIPT",
        &trigger,
        UiScriptLimits::default(),
    )
    .expect("the trigger member scans");
    assert_eq!(
        trigger_scan
            .sites
            .iter()
            .filter(|site| { site.form == DispatchForm::Callback && site.native_id == Some(2151) })
            .count(),
        1,
        "the trigger member's one 2151 site"
    );
    for fragment in [&b"FC != 0"[..], &b"\"campaignintro.script\""[..]] {
        assert!(
            trigger
                .windows(fragment.len())
                .any(|window| window == fragment),
            "the trigger member spells {}",
            String::from_utf8_lossy(fragment)
        );
    }

    // The denominator did not move: a video is not launchable content.
    assert!(
        baseline
            .roots
            .iter()
            .all(|id| id.kind() != ContentKind::Video),
        "the video collection is not a closure root"
    );
    assert_eq!(
        baseline.coverage.unreachable_by_kind.get("video"),
        Some(&5),
        "every video row is unreachable from a launchable root"
    );

    let report = baseline_report_json(&baseline);
    assert!(report.contains("\"chapter_name_unbound\":5"), "{report}");
    assert!(
        report.contains("\"chapter_source_unreadable\":0"),
        "{report}"
    );
}
