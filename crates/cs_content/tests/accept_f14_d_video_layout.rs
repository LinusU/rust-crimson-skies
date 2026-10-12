//! Acceptance scenario F14-D.VIDEO-LAYOUT: the second source of the `video`
//! collection of the retail baseline inventory (task #1351, stage `### F14-D`
//! of `specs/F14-canonical-content-catalog-and-dependency-closure.md`).
//!
//! F14-D.VIDEO (task #573) read the `PLAYAVI` actions of `ZBD/zrdr.zbd`'s
//! `fmv.zrd` member and counted every `.mpg` file no `PLAYAVI` name reached as
//! `unnamed_video_file`. Its review then decompressed the retail ROF
//! containers through the production reader and found `ASSETS/LAYOUT.CSV` of
//! `GOSDATA/ASSETS/crimson.rof` spelling `CrimFlag.MPG` five times and
//! `Final.MPG` once in `M` (movie) records — names the earlier raw scan could
//! not see because the member is zlib-compressed.
//!
//! **What this stage adds.** The layout member as a second source, read
//! through the production ROF mount and the keyed-list configuration reader.
//! A record is a video reference only when the measured schema classifies it
//! as a movie record (`RecordSchema::Layout(RecordKind::Movie)`) **and** its
//! file-name field — position 1 of the record value — states an `.mpg` name.
//! Rows appear only for names a record states, never from a bare file name;
//! each name resolves against the inventory exactly the way a `PLAYAVI` name
//! does, and the resolved row keeps the same identity, origin span and
//! readiness, with a distinct `f14.d.video.baseline.layout` claim whose
//! provenance source span is the record's own line inside the decoded member.
//!
//! **What this stage refuses.** A record that is not a movie record — even
//! one whose fields spell an `.mpg` name — is not a video reference. A movie
//! record whose file-name field is not an `.mpg` name is not a video
//! reference either and is counted under `movie_record_without_mpg_name`.
//! A source that cannot be read at all — an absent container, an absent
//! member or bytes that do not decode — is counted under
//! `unreadable_video_source` and named in the diagnostic rather than failing
//! the collection, because the other source's rows are real either way.
//! `CollectionStatus::source` names both containers the stage consults, so a
//! reader sees the whole source set even when one could not be read.
//!
//! The non-retail tests write **synthetic installation trees** into temporary
//! directories — including authored ROF containers, uncompressed and
//! zlib-compressed — that the production `retail_baseline` really reads.
//! Removing the layout wiring fails them.
//!
//! The retail test (`#[ignore = "requires CS_GAME_DIR"]`) reads the owner's
//! original installation and pins the five rows it really holds. Run it with
//! `--include-ignored`; without `CS_GAME_DIR` it fails loudly.
//!
//! Every member name and every byte below is authored for this file. No
//! original content is committed.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use cs_content::catalog::baseline::{
    Baseline, CollectionStatus, FMV_ARCHIVE, FMV_MEMBER, VIDEO_LAYOUT_CONTAINER,
    VIDEO_LAYOUT_MEMBER, baseline_report_json, install_file_key, retail_baseline,
};
use cs_formats::{DIRECTORY_HEADER_BYTES, FLAG_COMPRESSED, FLAG_DIRECTORY, RECORD_BYTES};
use cs_types::content::{
    CatalogElement, ContentId, ContentKind, NormalizeState, Readiness, UnsupportedReason,
};
use cs_types::evidence::ClaimStatus;
use cs_types::install::ParseState;

/// A disposable installation tree, removed on drop.
struct TempInstall(PathBuf);

impl TempInstall {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f14-d-video-layout-{label}-{}-{}",
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
    /// The stored extent: verbatim bytes for a plain member, the zlib stream
    /// for a compressed one.
    stored: Vec<u8>,
    /// The record's `raw_length` word: the decoded count — the member's own
    /// length for a plain member, the stream's output length for a
    /// compressed one.
    decoded_len: u32,
    flags: u32,
}

impl<'a> Entry<'a> {
    fn plain(name: &'a str, bytes: &[u8]) -> Self {
        Self {
            name,
            stored: bytes.to_vec(),
            decoded_len: bytes.len() as u32,
            flags: 0,
        }
    }

    /// The member stored the way the retail container stores
    /// `ASSETS/LAYOUT.CSV`: zlib-compressed, flag 2, the decoded count in the
    /// `raw_length` word.
    fn compressed(name: &'a str, bytes: &[u8]) -> Self {
        Self {
            name,
            stored: zlib_store(bytes),
            decoded_len: bytes.len() as u32,
            flags: FLAG_COMPRESSED,
        }
    }
}

/// The zlib stream of `bytes` as DEFLATE stored blocks — a legal RFC 1950
/// encoding the production bounded decoder inflates back to `bytes`.
fn zlib_store(bytes: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut chunks = bytes.chunks(u16::MAX as usize).peekable();
    while let Some(chunk) = chunks.next() {
        out.push(u8::from(chunks.peek().is_none()));
        let len = chunk.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    if bytes.is_empty() {
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xff, 0xff]);
    }
    out.extend_from_slice(&adler32(bytes).to_be_bytes());
    out
}

/// The Adler-32 checksum the zlib wrapper carries.
fn adler32(bytes: &[u8]) -> u32 {
    const MOD: u32 = 65_521;
    let (mut a, mut b) = (1_u32, 0_u32);
    for byte in bytes {
        a = (a + u32::from(*byte)) % MOD;
        b = (b + a) % MOD;
    }
    (b << 16) | a
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

/// `[directory: entries…][payloads…]`, one directory deep: the shape of the
/// retail archive's `ASSETS/` directory.
fn rof_container(directory: &str, entries: &[Entry<'_>]) -> Vec<u8> {
    let root_names = rof_names(&[directory]);
    let root_len = DIRECTORY_HEADER_BYTES + RECORD_BYTES + root_names.len();
    let entry_names: Vec<&str> = entries.iter().map(|entry| entry.name).collect();
    let sub_names = rof_names(&entry_names);
    let sub_len = DIRECTORY_HEADER_BYTES + entries.len() * RECORD_BYTES + sub_names.len();

    let root = rof_block(
        &[[
            root_len as u32,
            0,
            0,
            FLAG_DIRECTORY,
            directory.len() as u32 + 1,
            1,
        ]],
        &root_names,
    );
    let mut cursor = (root_len + sub_len) as u32;
    let mut records = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        records.push([
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
    bytes.extend_from_slice(&rof_block(&records, &sub_names));
    for entry in entries {
        bytes.extend_from_slice(&entry.stored);
    }
    assert_eq!(bytes.len(), cursor as usize, "every payload placed once");
    bytes
}

// -------------------------------------------------------------- trees -----

/// The per-mission members a scenario-shaped reader must list, so the
/// campaign walk finds the mission directory.
const MISSION_MEMBERS: [&str; 3] = ["map.zrd", "aiv.zrd", "objectives.zrd"];

/// A minimal installation: one campaign mission (so the shared campaign walk
/// finds a layout), the shared reader archive `FMV_ARCHIVE` holding
/// `fmv.zrd`, an optional `GOSDATA/ASSETS/crimson.rof` holding the layout
/// member, and any loose files the caller names.
fn tree(
    label: &str,
    fmv: Option<Vec<u8>>,
    layout: Option<Entry<'_>>,
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
    if let Some(member) = layout {
        temp.write(VIDEO_LAYOUT_CONTAINER, &rof_container("ASSETS", &[member]));
    }
    for (spelling, bytes) in loose {
        temp.write(spelling, bytes);
    }
    temp
}

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

/// Every reason a video row carries, as the claim ids or codes they report.
fn reasons(row: &CatalogElement) -> Vec<&'static str> {
    row.unsupported_reasons
        .iter()
        .map(UnsupportedReason::code)
        .collect()
}

/// The unknown reason's claim id, when the row carries one.
fn unknown_claim(row: &CatalogElement) -> Option<&str> {
    row.unsupported_reasons
        .iter()
        .find_map(|reason| match reason {
            UnsupportedReason::Unknown { claim_id, .. } => Some(claim_id.as_str()),
            _ => None,
        })
}

/// The video row whose resolved file is `spelling`, when there is one.
fn video_row_for<'a>(baseline: &'a Baseline, spelling: &str) -> Option<&'a CatalogElement> {
    rows_of(baseline, ContentKind::Video)
        .into_iter()
        .find(|row| row.id == cid(ContentKind::Video, &install_file_key(spelling)))
}

/// What every video row shares no matter which source named it: the file's
/// own identity, origin span, readiness shape and single static edge.
fn assert_video_row_shape(row: &CatalogElement, file: &str, bytes: &[u8], install_sha256: &str) {
    assert_eq!(row.id, cid(ContentKind::Video, &install_file_key(file)));
    assert!(row.origin.is_original());
    let span = row
        .origin
        .source()
        .expect("a video row is located by a span");
    assert_eq!(span.container_path(), file);
    assert_eq!(span.member_key(), None, "a loose file has no member");
    assert_eq!(span.offset(), 0);
    assert_eq!(span.length(), bytes.len() as u64);
    assert_eq!(span.install_sha256().to_hex(), install_sha256);
    assert_eq!(
        span.member_sha256().map(|digest| digest.to_hex()),
        Some(cs_assets::install::sha256(bytes).to_hex()),
        "the origin span's digest is the file's own bytes"
    );
    assert_eq!(row.parse_state, ParseState::Unparsed);
    assert_eq!(row.normalize_state, NormalizeState::NotNormalized);
    assert_eq!(row.readiness, Readiness::Unavailable);
    assert!(!row.is_ready());
    assert!(row.runtime_consumers.is_empty());
    assert_eq!(
        reasons(row),
        vec!["not_parsed", "unknown", "missing_runtime_consumer"]
    );
    assert_eq!(unknown_claim(row), Some("f14.d.video.playback"));
    assert_eq!(row.dependencies.len(), 1);
    assert_eq!(row.dependencies[0].kind.label(), "static");
    assert_eq!(
        row.dependencies[0].provenance.class,
        ClaimStatus::ObservedTool
    );
    assert_eq!(
        row.dependencies[0].target,
        cid(ContentKind::InstallFile, &install_file_key(file)),
        "the single static edge points at the named file's inventory row"
    );
}

// ------------------------------------------------------------ the tests ---

/// A movie record that states an `.mpg` name is one row under the layout
/// claim, with the record's own line as the edge provenance's span — beside
/// the `PLAYAVI` row of the other source, one status over both.
#[test]
fn accept_f14_d_video_layout_movie_records_become_rows() {
    // The member's own lines: a `;Movie Format`-style comment, two sections,
    // one record per shape the acceptance names.
    let member: &[u8] = b";Multimedia\n\
        [@CampaignIntro@]\n\
        MOVIE=M,CrimFlag.MPG,0,0,0,250,250,0,0\n\
        TITLE=T,other.mpg,0,0,0,1,1\n\
        [@FinalCinema@]\n\
        CF_MOVIE=M,Final.MPG,0,0,0,250,250,1,0\n\
        MOVIE=M,clip.wav,0,0,0,1,1,0,0\n\
        MOVIE=M,missing.mpg,0,0,0,1,1,0,0\n\
        MOVIE=M,CrimFlag.MPG,0,0,0,250,250,0,0\n";
    let temp = tree(
        "movie-rows",
        Some(fmv_document(&[("CHAP0", vec![playavi("chap0.mpg")])])),
        Some(Entry::plain("LAYOUT.CSV", member)),
        &[
            ("GOSDATA/ASSETS/GRAPHICS/MPG/chap0.mpg", b"chapter video"),
            ("GOSDATA/ASSETS/GRAPHICS/MPG/crimflag.mpg", b"flag video"),
            ("GOSDATA/ASSETS/GRAPHICS/MPG/final.mpg", b"final video"),
            // A loose `.mpg` only a *non-movie* record spells: not a video
            // reference, so the file is never promoted.
            ("GOSDATA/ASSETS/GRAPHICS/MPG/other.mpg", b"other video"),
            // A loose `.mpg` no record of either source names.
            (
                "GOSDATA/ASSETS/GRAPHICS/MPG/unreferenced.mpg",
                b"unreferenced",
            ),
        ],
    );
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    let rows = rows_of(&baseline, ContentKind::Video);
    assert_eq!(
        rows.len(),
        3,
        "chap0 via PLAYAVI, crimflag and final via layout"
    );

    // The `PLAYAVI` row keeps its own claim.
    let chap0 = video_row_for(&baseline, "GOSDATA/ASSETS/GRAPHICS/MPG/chap0.mpg")
        .expect("the PLAYAVI name resolves");
    assert_eq!(chap0.display_name.as_deref(), Some("chap0.mpg"));
    assert_eq!(
        chap0.dependencies[0].provenance.claim_id.as_str(),
        "f14.d.video.baseline.playavi"
    );
    assert_video_row_shape(
        chap0,
        "GOSDATA/ASSETS/GRAPHICS/MPG/chap0.mpg",
        b"chapter video",
        &baseline.install_sha256,
    );

    // The layout rows carry the layout claim, and the edge provenance's span
    // is the record's own line inside the decoded member — never the
    // resolved file's span.
    let container = fs::read(temp.0.join(VIDEO_LAYOUT_CONTAINER)).expect("the container reads");
    let member_offset = container.len() - member.len();
    for (file, spelled, record) in [
        (
            "GOSDATA/ASSETS/GRAPHICS/MPG/crimflag.mpg",
            "CrimFlag.MPG",
            &b"MOVIE=M,CrimFlag.MPG,0,0,0,250,250,0,0"[..],
        ),
        (
            "GOSDATA/ASSETS/GRAPHICS/MPG/final.mpg",
            "Final.MPG",
            &b"CF_MOVIE=M,Final.MPG,0,0,0,250,250,1,0"[..],
        ),
    ] {
        let row = video_row_for(&baseline, file).expect("the layout name resolves");
        assert_eq!(row.display_name.as_deref(), Some(spelled));
        assert_eq!(
            row.dependencies[0].provenance.claim_id.as_str(),
            "f14.d.video.baseline.layout"
        );
        let span = row.dependencies[0]
            .provenance
            .source
            .as_ref()
            .expect("the layout claim is located by a span");
        assert_eq!(span.container_path(), VIDEO_LAYOUT_CONTAINER);
        assert_eq!(span.member_key(), Some(VIDEO_LAYOUT_MEMBER));
        assert_eq!(span.install_sha256().to_hex(), baseline.install_sha256);
        assert_eq!(span.length(), record.len() as u64);
        assert_eq!(
            span.member_sha256().map(|digest| digest.to_hex()),
            Some(cs_assets::install::sha256(member).to_hex()),
            "the provenance span's digest is the decoded member's bytes"
        );
        // The member is stored uncompressed, so the span's offset is the
        // line's own position in the container file.
        let line_at = member
            .windows(record.len())
            .position(|window| window == record)
            .expect("the record line is in the member");
        assert_eq!(span.offset(), (member_offset + line_at) as u64);
        assert_eq!(
            &container[span.offset() as usize..span.offset() as usize + span.length() as usize],
            record,
            "the span locates the record's own bytes"
        );
        assert_video_row_shape(
            row,
            file,
            match spelled {
                "CrimFlag.MPG" => b"flag video",
                _ => b"final video",
            },
            &baseline.install_sha256,
        );
    }

    let status = status_of(&baseline, ContentKind::Video);
    assert_eq!(
        status.source,
        format!("{FMV_ARCHIVE} + {VIDEO_LAYOUT_CONTAINER}"),
        "the status names both contributors"
    );
    assert_eq!(status.language, None);
    assert_eq!(status.rows, 3);
    assert_eq!(
        status.gaps,
        BTreeMap::from([
            ("playavi_name_not_text", 0),
            // `MOVIE=M,clip.wav,...`: a movie record whose value is not an
            // `.mpg` name is not a video reference.
            ("movie_record_without_mpg_name", 1),
            // `MOVIE=M,missing.mpg,...`: no inventoried file of that name.
            ("named_file_absent", 1),
            ("ambiguous_file_name", 0),
            // The second `CrimFlag.MPG` record resolves to the file already
            // rowed, so the repeat is counted.
            ("duplicate_video_name", 1),
            // `other.mpg` is spelled only by a non-movie record and
            // `unreferenced.mpg` by nothing.
            ("unnamed_video_file", 2),
            ("unreadable_video_source", 0),
        ])
    );
    assert_eq!(status.diagnostic, None);

    // The denominator did not move: a video is not launchable content.
    assert_eq!(baseline.roots, vec![cid(ContentKind::Mission, "ch1-m01")]);
    assert_eq!(baseline.coverage.roots, 1);
    assert_eq!(baseline.catalog.launchable_count(), 1);
    assert_eq!(baseline.coverage.unreachable_by_kind.get("video"), Some(&3));
    let report = baseline_report_json(&baseline);
    assert!(report.contains("\"video\":3"), "{report}");
    assert!(report.contains("\"duplicate_video_name\":1"), "{report}");
}

/// The member stored the way the retail container stores it — zlib-compressed
/// — is read through the production decoder, and the provenance span's digest
/// is the decoded bytes'.
#[test]
fn accept_f14_d_video_layout_compressed_member_is_read() {
    let member: &[u8] = b"[@MainMenu@]\nMOVIE=M,CrimFlag.MPG,0,0,0,250,250,0,0\n";
    let temp = tree(
        "compressed",
        Some(fmv_document(&[("CHAP0", vec![playavi("chap0.mpg")])])),
        Some(Entry::compressed("LAYOUT.CSV", member)),
        &[("GOSDATA/ASSETS/GRAPHICS/MPG/crimflag.mpg", b"flag video")],
    );
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    let row = video_row_for(&baseline, "GOSDATA/ASSETS/GRAPHICS/MPG/crimflag.mpg")
        .expect("the compressed member's movie record names the video");
    let span = row.dependencies[0]
        .provenance
        .source
        .as_ref()
        .expect("the layout claim is located by a span");
    assert_eq!(span.container_path(), VIDEO_LAYOUT_CONTAINER);
    assert_eq!(span.member_key(), Some(VIDEO_LAYOUT_MEMBER));
    assert_eq!(
        span.member_sha256().map(|digest| digest.to_hex()),
        Some(cs_assets::install::sha256(member).to_hex()),
        "the digest covers the decoded bytes, not the stored stream"
    );

    let status = status_of(&baseline, ContentKind::Video);
    assert_eq!(status.rows, 1);
    assert_eq!(status.gaps["unreadable_video_source"], 0);
    assert_eq!(status.diagnostic, None);
}

/// A layout member that states no movie record naming an `.mpg` file yields
/// no row from itself — and the `PLAYAVI` row beside it still builds, because
/// one source's silence never erases the other's rows.
#[test]
fn accept_f14_d_video_layout_no_movie_record_names_nothing() {
    let member: &[u8] = b";Multimedia\n\
        [@MainMenu@]\n\
        TITLE=T,other.mpg,0,0,0,1,1\n\
        PANE=P,art.png,0,0,0,1,1\n";
    let temp = tree(
        "no-movie",
        Some(fmv_document(&[("CHAP0", vec![playavi("chap0.mpg")])])),
        Some(Entry::plain("LAYOUT.CSV", member)),
        &[
            ("GOSDATA/ASSETS/GRAPHICS/MPG/chap0.mpg", b"chapter video"),
            ("GOSDATA/ASSETS/GRAPHICS/MPG/other.mpg", b"other video"),
        ],
    );
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    let rows = rows_of(&baseline, ContentKind::Video);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].dependencies[0].provenance.claim_id.as_str(),
        "f14.d.video.baseline.playavi"
    );

    let status = status_of(&baseline, ContentKind::Video);
    assert_eq!(
        status.gaps,
        BTreeMap::from([
            ("playavi_name_not_text", 0),
            ("movie_record_without_mpg_name", 0),
            ("named_file_absent", 0),
            ("ambiguous_file_name", 0),
            ("duplicate_video_name", 0),
            // `other.mpg` is spelled only by a non-movie record.
            ("unnamed_video_file", 1),
            ("unreadable_video_source", 0),
        ])
    );
    assert_eq!(status.diagnostic, None);
}

/// An installation without the ROF container keeps the `PLAYAVI` rows: the
/// missing source is counted under `unreadable_video_source` and named in the
/// diagnostic rather than failing the collection or erasing the other
/// source's rows.
#[test]
fn accept_f14_d_video_layout_absent_container_keeps_fmv_rows() {
    let temp = tree(
        "no-rof",
        Some(fmv_document(&[("CHAP0", vec![playavi("chap0.mpg")])])),
        None,
        &[("GOSDATA/ASSETS/GRAPHICS/MPG/chap0.mpg", b"chapter video")],
    );
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    let rows = rows_of(&baseline, ContentKind::Video);
    assert_eq!(
        rows.len(),
        1,
        "the PLAYAVI row does not depend on the layout source"
    );
    assert_eq!(
        rows[0].dependencies[0].provenance.claim_id.as_str(),
        "f14.d.video.baseline.playavi"
    );

    let status = status_of(&baseline, ContentKind::Video);
    assert_eq!(status.rows, 1);
    assert_eq!(status.gaps["unreadable_video_source"], 1);
    let diagnostic = status
        .diagnostic
        .as_deref()
        .expect("a source that could not be read is named");
    assert!(diagnostic.contains(VIDEO_LAYOUT_CONTAINER), "{diagnostic}");
}

// --------------------------------------------------------------- retail ---

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

/// The row counts the retail acceptance pins, measured on the owner's
/// installation and recorded in
/// `docs/findings/2026-10-12-f14-d-video-layout-collection.md`.
///
/// `ZBD/zrdr.zbd`'s `fmv.zrd` names `MSopen1.mpg`, `zipper.mpg` and
/// `Chap0.mpg`; `GOSDATA/ASSETS/crimson.rof`'s `ASSETS/LAYOUT.CSV` member —
/// read decompressed through the production ROF reader — states
/// `MOVIE=M,CrimFlag.MPG,…` five times (three `MOVIE` records, one
/// `CM_MOVIE` and one under the `;Multimedia` block) and
/// `CF_MOVIE=M,Final.MPG,…` once. The first `CrimFlag.MPG` record resolves to
/// the file, the four repeats are counted duplicates, and the five chapter
/// files `chap1`…`chap5` — *constructed* by
/// `ASSETS/SCRIPTS/CAMPAIGNINTRO.SCRIPT`, a binding Rally #1352 owns — stay
/// counted `unnamed_video_file`.
const RETAIL_VIDEO_ROWS: usize = 5;
const RETAIL_UNNAMED_MPG_FILES: usize = 5;
const RETAIL_DUPLICATE_NAMES: usize = 4;

/// The retail half: five files become rows — three under the `PLAYAVI`
/// claim, two under the layout claim with provenance spans inside the
/// decoded member — five stay unnamed, and the coverage denominator is
/// unchanged.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f14_d_video_layout_retail_movie_records_are_rows() {
    let dir = game_dir();
    let baseline = retail_baseline(&dir).expect("the original installation reads");
    let catalog = &baseline.catalog;

    let rows = rows_of(&baseline, ContentKind::Video);
    assert_eq!(
        rows.len(),
        RETAIL_VIDEO_ROWS,
        "the five files the two sources name are the five rows; a different count must be \
         re-measured and this test updated, never silently accepted"
    );

    // The decoded layout member, read again through the production ROF
    // mount, so the provenance spans can be checked against the member's own
    // stored offset and decoded digest — never against a fixture.
    let mut builder = cs_assets::vfs::SessionBuilder::new(cs_types::asset_id::ResolveContext::new(
        cs_types::evidence::ContentHash::from_hex(&baseline.install_sha256)
            .expect("the reported fingerprint is a content hash"),
    ));
    let mount = cs_assets::vfs::MountBuilder::new(
        cs_types::asset_id::MountId::new("rof-video-layout-check").expect("valid mount id"),
        cs_types::asset_id::MountNamespace::new(cs_assets::vfs::INSTALL_NAMESPACE)
            .expect("valid namespace"),
        cs_types::asset_id::PrecedenceClass::Shared,
        VIDEO_LAYOUT_CONTAINER,
    )
    .retail();
    let source =
        cs_assets::rof::mount_rof_into(&mut builder, mount, &dir.join(VIDEO_LAYOUT_CONTAINER))
            .expect("the retail shared archive mounts");
    let key = cs_types::asset_id::AssetKey::from_spelling(
        source.namespace().as_str(),
        VIDEO_LAYOUT_MEMBER,
        "default",
    )
    .expect("the member key is valid");
    let info = source
        .member(&key)
        .cloned()
        .expect("the layout member is in the container");
    let member = source.read(&key).expect("the layout member reads").data;
    let member_sha256 = cs_assets::install::sha256(&member);

    let mut named = Vec::new();
    for row in &rows {
        let file = row
            .origin
            .source()
            .expect("a video row is located by a span")
            .container_path()
            .to_owned();
        assert_video_row_shape(
            row,
            &file,
            &fs::read(dir.join(&file)).expect("the named file reads"),
            &baseline.install_sha256,
        );
        named.push(
            row.display_name
                .as_deref()
                .expect("a video row carries the record's spelling")
                .to_ascii_lowercase(),
        );

        let claim = row.dependencies[0].provenance.claim_id.as_str();
        let span = row.dependencies[0]
            .provenance
            .source
            .as_ref()
            .expect("every edge is located by a span");
        match file.as_str() {
            "GOSDATA/ASSETS/GRAPHICS/MPG/crimflag.mpg"
            | "GOSDATA/ASSETS/GRAPHICS/MPG/final.mpg" => {
                assert_eq!(
                    claim, "f14.d.video.baseline.layout",
                    "{file}: the layout record is the observation"
                );
                assert_eq!(span.container_path(), VIDEO_LAYOUT_CONTAINER);
                assert_eq!(span.member_key(), Some(VIDEO_LAYOUT_MEMBER));
                assert_eq!(
                    span.member_sha256(),
                    Some(member_sha256),
                    "{file}: the span digests the decoded member"
                );
                // The offset is the record line's position inside the
                // decoded member, anchored at the member's stored extent.
                assert!(
                    span.offset() >= info.offset
                        && span.offset() + span.length() <= info.offset + info.declared_decoded_len,
                    "{file}: the record span is inside the member's decoded extent"
                );
            }
            _ => {
                assert_eq!(
                    claim, "f14.d.video.baseline.playavi",
                    "{file}: the PLAYAVI action is the observation"
                );
                assert_eq!(
                    span.container_path(),
                    file.as_str(),
                    "{file}: the PLAYAVI provenance keeps the resolved file's span"
                );
            }
        }
    }
    named.sort();
    assert_eq!(
        named,
        vec![
            "chap0.mpg",
            "crimflag.mpg",
            "final.mpg",
            "msopen1.mpg",
            "zipper.mpg"
        ]
    );

    let status = status_of(&baseline, ContentKind::Video);
    assert_eq!(
        status.source,
        format!("{FMV_ARCHIVE} + {VIDEO_LAYOUT_CONTAINER}")
    );
    assert_eq!(status.language, None);
    assert_eq!(status.rows, RETAIL_VIDEO_ROWS);
    assert_eq!(
        status.gaps,
        BTreeMap::from([
            ("playavi_name_not_text", 0),
            ("movie_record_without_mpg_name", 0),
            ("named_file_absent", 0),
            ("ambiguous_file_name", 0),
            // The first `CrimFlag.MPG` record makes the row; the other four
            // repeats are counted.
            ("duplicate_video_name", RETAIL_DUPLICATE_NAMES),
            // `chap1`…`chap5`: constructed by CAMPAIGNINTRO.SCRIPT, owned by
            // Rally #1352.
            ("unnamed_video_file", RETAIL_UNNAMED_MPG_FILES),
            ("unreadable_video_source", 0),
        ]),
        "the five .mpg files no record of either source reaches are the only \
         unaccounted-for entries; a new gap code here means the measured \
         sources changed and this measurement has to be re-measured"
    );
    assert_eq!(status.diagnostic, None);

    // The denominator did not move: a video is not launchable content.
    assert!(!ContentKind::Video.is_launchable());
    assert!(
        baseline
            .roots
            .iter()
            .all(|id| id.kind() != ContentKind::Video),
        "the video collection is not a closure root"
    );
    assert_eq!(catalog.launchable_count(), baseline.roots.len());
    assert_eq!(catalog.synthetic_launchable_count(), 0);
    assert_eq!(baseline.coverage.roots, baseline.roots.len());
    assert_eq!(baseline.coverage.unresolved_references, 0);
    assert_eq!(
        baseline.coverage.unreachable_by_kind.get("video"),
        Some(&RETAIL_VIDEO_ROWS),
        "every video row is unreachable from a launchable root"
    );

    let report = baseline_report_json(&baseline);
    assert!(
        report.contains(&format!("\"video\":{RETAIL_VIDEO_ROWS}")),
        "{report}"
    );
    assert!(
        report.contains(&format!(
            "\"unnamed_video_file\":{RETAIL_UNNAMED_MPG_FILES}"
        )),
        "{report}"
    );
    assert!(
        !report.contains("\"origin\":\"synthetic_fixture\""),
        "{report}"
    );
}
