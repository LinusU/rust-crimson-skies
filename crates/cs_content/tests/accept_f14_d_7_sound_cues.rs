//! Acceptance scenario F14-D.7: the `sound`, `music` and `dialogue` collections
//! of the retail baseline inventory (task #490, stage `### F14-D` of
//! `specs/F14-canonical-content-catalog-and-dependency-closure.md`).
//!
//! `docs/contracts/IDENTITY-CONTENT.md` requires "sounds/music/dialogue/video"
//! as catalog collections. The baseline inventory had no `ContentKind::Sound`
//! row at all; the only audio catalog in the tree is
//! `cs_content::audio`'s **declared synthetic** one (F41-A), which is authored
//! content and cannot be mistaken for original data.
//!
//! **What this stage adds.** One bounded container family: the ZBD sound family
//! (`ZBD/sounds*.zbd`), which is the only audio container this installation
//! holds. For every container the producing stage's own role rule names, the
//! container's own trailer member index is read and the F06 sound reader lists
//! its members. A `ContentKind::Sound` row exists **only** for a member whose
//! declared extent lies inside the container and whose RIFF/WAVE header reads.
//! The identity is the container plus the name that container's own index
//! declares; the span is the member's own extent carrying the container path,
//! the member key and the member's digest; the single static edge points at the
//! inventory row of the container.
//!
//! **What this stage refuses.** A cue named only by a file name is not a row: a
//! member whose header does not read is a gap under that header reader's own
//! code. A name one container declares twice with different bytes has no
//! identity that tells the two apart, so neither is a row
//! (`ambiguous_member_name`). A name declared twice with **identical** bytes is
//! one cue and each repeat is counted (`duplicate_member`) — one identity, one
//! cue, explicitly resolved rather than silently duplicated or filtered out.
//! Nothing in a member's bytes separates a music cue or a spoken line from any
//! other cue, so `music` and `dialogue` hold no row and say why in their own
//! `CollectionStatus` records instead of being minted from a member-name prefix.
//!
//! The non-retail tests write **synthetic installation trees** into temporary
//! directories, whose reader archives and sound containers are built the way the
//! pinned format readers expect, so the production `retail_baseline` really
//! reads them. Removing the sound collection fails them.
//!
//! The retail test (`#[ignore = "requires CS_GAME_DIR"]`) reads the owner's
//! original installation and pins the rows it really holds. Run it with
//! `--include-ignored`; without `CS_GAME_DIR` it fails loudly.
//!
//! Every member name and every byte below is authored for this file. No
//! original content is committed.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cs_content::catalog::baseline::{
    Baseline, CollectionStatus, SOUND_CONTAINER_PATTERN, baseline_report_json, install_file_key,
    retail_baseline,
};
use cs_types::content::{
    CatalogElement, ContentId, ContentKind, NormalizeState, Origin, Readiness, UnsupportedReason,
};
use cs_types::evidence::ClaimStatus;
use cs_types::install::ParseState;

/// A disposable installation tree, removed on drop.
struct TempInstall(PathBuf);

impl TempInstall {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f14-d-7-{label}-{}-{}",
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

/// The low-rate sound container, spelled as the installation spells it.
const LOW: &str = "ZBD/soundsl.zbd";
/// The high-rate sound container.
const HIGH: &str = "ZBD/soundsh.zbd";

/// The per-mission members a scenario-shaped reader must list, so the campaign
/// walk finds the mission directory.
const MISSION_MEMBERS: [&str; 3] = ["map.zrd", "aiv.zrd", "objectives.zrd"];

// ------------------------------------------------------- a WAVE member ----

/// One authored RIFF chunk: id, size, payload, and the pad byte an odd payload
/// needs (IBM/Microsoft 1991, "RIFF File Format").
fn chunk(id: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut bytes = id.to_vec();
    bytes.extend_from_slice(&(u32::try_from(payload.len()).expect("fits")).to_le_bytes());
    bytes.extend_from_slice(payload);
    if payload.len() % 2 == 1 {
        bytes.push(0);
    }
    bytes
}

/// A `fmt ` payload: the 16 common bytes, then the format-specific tail.
fn fmt_chunk(rate: u32, extra: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1u16.to_le_bytes()); // wFormatTag: WAVE_FORMAT_PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // nChannels
    bytes.extend_from_slice(&rate.to_le_bytes()); // nSamplesPerSec
    bytes.extend_from_slice(&rate.to_le_bytes()); // nAvgBytesPerSec
    bytes.extend_from_slice(&1u16.to_le_bytes()); // nBlockAlign
    bytes.extend_from_slice(&8u16.to_le_bytes()); // wBitsPerSample
    bytes.extend_from_slice(extra);
    bytes
}

/// `RIFF`, its size, `WAVE` and `chunks`.
fn wave(chunks: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = chunks.concat();
    let mut bytes = b"RIFF".to_vec();
    bytes.extend_from_slice(&(u32::try_from(body.len()).expect("fits") + 4).to_le_bytes());
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(&body);
    bytes
}

/// One 8-bit mono PCM recording of `samples` at `rate`, plus a `cue ` chunk
/// when `points` is non-zero, so a fixture member can carry the cue point the
/// retail briefing recordings declare.
fn pcm_member(rate: u32, samples: &[u8], points: u32) -> Vec<u8> {
    let mut chunks = vec![
        chunk(b"fmt ", &fmt_chunk(rate, &[])),
        chunk(b"data", samples),
    ];
    if points > 0 {
        let mut cue = points.to_le_bytes().to_vec();
        for point in 0..points {
            for word in [point + 1, point * 100] {
                cue.extend_from_slice(&word.to_le_bytes());
            }
            cue.extend_from_slice(b"data");
            for word in [0u32, 0, point * 100] {
                cue.extend_from_slice(&word.to_le_bytes());
            }
        }
        // `cue ` precedes `data` in the retail members; chunk order does not
        // matter to the reader, only that `data` comes after `fmt `.
        chunks = vec![
            chunk(b"fmt ", &fmt_chunk(rate, &[])),
            chunk(b"cue ", &cue),
            chunk(b"data", samples),
        ];
    }
    wave(&chunks)
}

// ------------------------------------------------- a sound container -------

/// One declared member of a fixture sound archive.
enum FixtureMember<'a> {
    /// A member whose bytes are stored back to back, starting where the last
    /// stored member ended.
    Bytes {
        /// The name its own index entry declares.
        name: &'a str,
        /// The member's bytes.
        bytes: &'a [u8],
    },
    /// A member whose declared extent is exactly this one, whatever the layout
    /// stores. Used for the member that reaches past the member data.
    Extent {
        /// The name its own index entry declares.
        name: &'a str,
        /// Declared start offset.
        start: u32,
        /// Declared length.
        length: u32,
    },
}

/// A sound container laid out the way the version-one trailer reader expects
/// (task #343): member data first, then one 148-byte entry per member, then the
/// version word and the member count.
fn sound_archive(members: &[FixtureMember<'_>]) -> Vec<u8> {
    let mut data = Vec::new();
    let mut entries = Vec::new();
    for member in members {
        let (name, start, length) = match member {
            FixtureMember::Bytes { name, bytes } => {
                let start = u32::try_from(data.len()).expect("a fixture member fits");
                let length = u32::try_from(bytes.len()).expect("a fixture member fits");
                data.extend_from_slice(bytes);
                (*name, start, length)
            }
            FixtureMember::Extent {
                name,
                start,
                length,
            } => (*name, *start, *length),
        };
        entries.extend_from_slice(&start.to_le_bytes());
        entries.extend_from_slice(&length.to_le_bytes());
        let mut field = vec![0u8; 64];
        field[..name.len()].copy_from_slice(name.as_bytes());
        entries.extend_from_slice(&field);
        entries.extend_from_slice(&[0u8; 76]);
    }
    data.extend_from_slice(&entries);
    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&u32::try_from(members.len()).expect("fits").to_le_bytes());
    data
}

/// The same container builder as [`sound_archive`], over plain members.
fn archive(members: &[(&str, &[u8])]) -> Vec<u8> {
    let members: Vec<FixtureMember<'_>> = members
        .iter()
        .map(|(name, bytes)| FixtureMember::Bytes { name, bytes })
        .collect();
    sound_archive(&members)
}

// ----------------------------------------------------------- the tree ------

/// A minimal installation: one campaign mission (so the shared campaign walk
/// finds a layout) plus the sound containers the caller names.
fn tree(label: &str, containers: &[(&str, Vec<u8>)]) -> TempInstall {
    let temp = TempInstall::new(label);
    temp.write(
        "ZBD/C1C/M01/zrdr.zbd",
        &archive(&[
            ("net.zrd", b"shared"),
            (MISSION_MEMBERS[0], b"map"),
            (MISSION_MEMBERS[1], b"aiv"),
            (MISSION_MEMBERS[2], b"objectives"),
        ]),
    );
    temp.write("ZBD/C1C/M01/mis_anim.zbd", b"mission animation bytes");
    for (spelling, bytes) in containers {
        temp.write(spelling, bytes);
    }
    temp
}

// -------------------------------------------------------------- mapping ----

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("test content id is valid")
}

/// The id of the cue `container` declares under `name`.
fn cue_id(container: &str, name: &str) -> ContentId {
    cid(
        ContentKind::Sound,
        &install_file_key(&format!("{container}/{name}")),
    )
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

/// Every reason a sound row carries, as the claim ids or codes they report.
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

// ------------------------------------------------------------ the mapping --

/// A readable member becomes a `sound` row: identity is the container plus the
/// name its own index declares, the span names the member's own bytes, and the
/// one static edge points at the container's inventory row. F41's declared
/// playback metadata stays an explicit unknown and nothing claims a consumer.
#[test]
fn accept_f14_d_7_a_readable_member_yields_a_sound_row() {
    let gun = pcm_member(11_025, &[7, 9, 11], 0);
    let narration = pcm_member(11_025, &[1, 2, 3, 4], 2);
    let temp = tree(
        "members",
        &[(
            LOW,
            archive(&[
                ("ab_mgun1.wav", &gun),
                ("c2-NW-m1_briefing.wav", &narration),
                // Not a recording: the header does not read.
                ("broken.wav", b"this is not a wave file"),
            ]),
        )],
    );
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    let rows = rows_of(&baseline, ContentKind::Sound);
    assert_eq!(rows.len(), 2, "one row per readable member, not per name");
    assert_eq!(
        rows.iter().map(|row| row.id.clone()).collect::<Vec<_>>(),
        vec![
            cue_id(LOW, "ab_mgun1.wav"),
            cue_id(LOW, "c2-NW-m1_briefing.wav"),
        ],
        "identity is the container plus the declared member name, in canonical order"
    );
    assert!(
        baseline
            .catalog
            .get(&cid(ContentKind::Sound, &install_file_key("ab_mgun1.wav")))
            .is_none(),
        "a bare member name is not an identity: the same name is declared in both \
         sound containers with different bytes"
    );

    let container_row = cid(ContentKind::InstallFile, &install_file_key(LOW));
    let blob = fs::read(temp.0.join(LOW)).expect("the container bytes read");
    for (row, (name, member)) in rows.iter().zip([
        ("ab_mgun1.wav", &gun),
        ("c2-NW-m1_briefing.wav", &narration),
    ]) {
        assert!(matches!(row.origin, Origin::Installation { .. }));
        assert_eq!(row.display_name.as_deref(), Some(name));
        let span = row.origin.source().expect("an installation span");
        assert_eq!(span.container_path(), LOW);
        assert_eq!(span.member_key(), Some(name));
        assert_eq!(span.install_sha256().to_hex(), baseline.install_sha256);
        assert!(span.length() > 0, "{name}: a member has bytes");
        let offset = usize::try_from(span.offset()).expect("fits");
        let length = usize::try_from(span.length()).expect("fits");
        assert_eq!(
            &blob[offset..offset + length],
            &member[..],
            "{name}: the span names the member's own stored bytes"
        );
        assert_eq!(
            span.member_sha256().map(|digest| digest.to_hex()),
            Some(cs_assets::install::sha256(member).to_hex()),
            "{name}: the member is hashed"
        );
        assert_eq!(
            row.fingerprint.as_ref().map(|digest| digest.sha256),
            Some(cs_assets::install::sha256(member)),
            "{name}: the row fingerprints the member's bytes"
        );

        // The header was read, so the member is parsed; nothing is normalized.
        assert_eq!(row.parse_state, ParseState::Parsed);
        assert_eq!(row.normalize_state, NormalizeState::NotNormalized);
        assert_eq!(row.readiness, Readiness::Unavailable);
        assert!(!row.is_ready());
        assert_eq!(
            reasons(row),
            vec!["not_normalized", "unknown"],
            "{name}: playback metadata is an explicit unknown, not a default"
        );
        assert_eq!(
            unknown_claim(row),
            Some("f14.d.7.baseline.sound_playback"),
            "{name}: F41's declared bus/playback metadata stays unknown"
        );
        assert!(
            row.runtime_consumers.is_empty(),
            "{name}: no media player consumer is claimed without evidence"
        );

        assert_eq!(row.dependencies.len(), 1);
        assert_eq!(row.dependencies[0].target, container_row);
        assert_eq!(row.dependencies[0].kind.label(), "static");
        assert_eq!(
            row.dependencies[0].provenance.claim_id.as_str(),
            "f14.d.7.baseline.sound_member"
        );
        assert_eq!(
            row.dependencies[0].provenance.class,
            ClaimStatus::ObservedTool,
            "an agent-observed edge is never verified_original"
        );
    }

    let status = status_of(&baseline, ContentKind::Sound);
    assert_eq!(status.source, SOUND_CONTAINER_PATTERN);
    assert_eq!(status.language, None);
    assert_eq!(status.rows, 2);
    assert_eq!(
        status.gaps.get("not_riff"),
        Some(&1),
        "the member whose header does not read is a named gap, not a dropped row"
    );
    assert_eq!(status.diagnostic, None);

    let report = baseline_report_json(&baseline);
    assert!(report.contains("\"sound\":2"), "{report}");
    assert!(report.contains("\"not_riff\":1"), "{report}");
    assert!(
        report.contains(&format!(
            "\"kind\":\"sound\",\"source\":\"{SOUND_CONTAINER_PATTERN}\",\"language\":null,\"rows\":2"
        )),
        "{report}"
    );
    assert!(
        !report.contains("\"origin\":\"synthetic_fixture\""),
        "the retail consumer report holds no authored row"
    );
    assert_eq!(
        report,
        baseline_report_json(&retail_baseline(&temp.0).expect("re-read")),
        "the report is byte-stable for the same installation"
    );
}

/// The same cue name in both sound containers is two rows with two spans, so a
/// low-rate and a high-rate recording of one cue never merge.
#[test]
fn accept_f14_d_7_a_one_name_in_both_containers_is_two_rows_with_two_spans() {
    let low = pcm_member(11_025, &[1, 2, 3], 0);
    let high = pcm_member(22_050, &[9, 8, 7, 6, 5, 4], 0);
    let temp = tree(
        "both",
        &[
            (LOW, archive(&[("fury.wav", &low)])),
            (HIGH, archive(&[("fury.wav", &high)])),
        ],
    );
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    let rows = rows_of(&baseline, ContentKind::Sound);
    assert_eq!(rows.len(), 2, "one row per container, not one per name");
    assert_eq!(
        rows.iter().map(|row| row.id.clone()).collect::<Vec<_>>(),
        vec![cue_id(HIGH, "fury.wav"), cue_id(LOW, "fury.wav")],
        "the container is part of the identity, and rows come out in canonical id order"
    );
    let spans: Vec<(String, String, u64)> = rows
        .iter()
        .map(|row| {
            let span = row.origin.source().expect("an installation span");
            (
                span.container_path().to_owned(),
                span.member_key().expect("a member key").to_owned(),
                span.length(),
            )
        })
        .collect();
    assert_eq!(
        spans,
        vec![
            (HIGH.to_owned(), "fury.wav".to_owned(), high.len() as u64),
            (LOW.to_owned(), "fury.wav".to_owned(), low.len() as u64),
        ],
        "each row names the container its own bytes live in"
    );
    assert_eq!(
        rows[0].dependencies[0].target,
        cid(ContentKind::InstallFile, &install_file_key(HIGH))
    );
    assert_eq!(
        rows[1].dependencies[0].target,
        cid(ContentKind::InstallFile, &install_file_key(LOW))
    );

    let status = status_of(&baseline, ContentKind::Sound);
    assert_eq!(status.rows, 2);
    assert!(
        status.gaps.is_empty(),
        "two containers declaring one name is not a duplicate: {:?}",
        status.gaps
    );
}

/// A name one container declares several times with identical bytes is one cue
/// with the repeats counted; with different bytes no member is a row at all,
/// because the index gives no identity that tells them apart.
#[test]
fn accept_f14_d_7_a_repeated_name_is_one_cue_or_no_cue_and_never_two_rows() {
    let same = pcm_member(11_025, &[1, 2, 3], 0);
    let other = pcm_member(22_050, &[4, 5, 6], 0);
    let temp = tree(
        "repeated",
        &[(
            LOW,
            sound_archive(&[
                FixtureMember::Bytes {
                    name: "id47_CTF-Stolen.wav",
                    bytes: &same,
                },
                FixtureMember::Bytes {
                    name: "id47_CTF-Stolen.wav",
                    bytes: &same,
                },
                // The same identity under a different letter case: the id grammar
                // folds case, so this is the same name with the same bytes.
                FixtureMember::Bytes {
                    name: "id47_CTF-STOLEN.wav",
                    bytes: &same,
                },
                FixtureMember::Bytes {
                    name: "siren.wav",
                    bytes: &other,
                },
                FixtureMember::Bytes {
                    name: "siren.wav",
                    bytes: &same,
                },
            ]),
        )],
    );
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    let rows = rows_of(&baseline, ContentKind::Sound);
    assert_eq!(
        rows.iter().map(|row| row.id.clone()).collect::<Vec<_>>(),
        vec![cue_id(LOW, "id47_CTF-Stolen.wav")],
        "one identity, one row: identical repeats merge and differing bytes are no cue"
    );
    let status = status_of(&baseline, ContentKind::Sound);
    assert_eq!(status.rows, 1);
    assert_eq!(
        status.gaps.get("duplicate_member"),
        Some(&2),
        "the two byte-identical repeats are counted, never dropped silently"
    );
    assert_eq!(
        status.gaps.get("ambiguous_member_name"),
        Some(&2),
        "two different recordings under one name have no identity to key"
    );

    // The surviving row points at the first declared occurrence, deterministically.
    let first = same.as_slice();
    let span = rows[0]
        .origin
        .source()
        .expect("the surviving row is located");
    assert_eq!(span.member_key(), Some("id47_CTF-Stolen.wav"));
    let blob = fs::read(temp.0.join(LOW)).expect("the container bytes read");
    let offset = usize::try_from(span.offset()).expect("fits");
    let length = usize::try_from(span.length()).expect("fits");
    assert_eq!(&blob[offset..offset + length], first);
}

/// The sound family names every member as a cue and stores a recording, so
/// `music` and `dialogue` hold no row: a member-name prefix is not a cue class.
#[test]
fn accept_f14_d_7_music_and_dialogue_hold_no_row_and_say_why() {
    let music = pcm_member(11_025, &[1, 2], 0);
    let voice = pcm_member(11_025, &[3, 4], 0);
    let temp = tree(
        "classes",
        &[(
            LOW,
            archive(&[
                ("music_missionsuccess1.wav", &music),
                ("VO_c3-HW-m1_CharlieSteele_7.wav", &voice),
            ]),
        )],
    );
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    for name in [
        "music_missionsuccess1.wav",
        "VO_c3-HW-m1_CharlieSteele_7.wav",
    ] {
        let row = baseline
            .catalog
            .get(&cue_id(LOW, name))
            .unwrap_or_else(|| panic!("{name} is a sound cue"));
        assert_eq!(row.kind, ContentKind::Sound, "{name} stays a sound cue");
    }
    for kind in [ContentKind::Music, ContentKind::Dialogue] {
        assert!(rows_of(&baseline, kind).is_empty());
        let status = status_of(&baseline, kind);
        assert_eq!(status.rows, 0);
        assert_eq!(status.source, SOUND_CONTAINER_PATTERN);
        assert_eq!(status.language, None);
        let diagnostic = status
            .diagnostic
            .as_deref()
            .unwrap_or_else(|| panic!("the {kind} collection says why it holds no row"));
        assert!(diagnostic.contains("cue"), "{diagnostic}");
    }

    let report = baseline_report_json(&baseline);
    assert!(report.contains("\"kind\":\"music\""), "{report}");
    assert!(report.contains("\"kind\":\"dialogue\""), "{report}");
    assert!(!report.contains("\"music\":"), "{report}");
    assert!(!report.contains("\"dialogue\":"), "{report}");
}

/// A member whose declared extent is not inside the container, or whose name is
/// empty, is a named gap: it is in the index, so it must be accounted for, and
/// it has no bytes that could back a cue.
#[test]
fn accept_f14_d_7_a_member_with_no_readable_bytes_is_a_named_gap() {
    let good = pcm_member(11_025, &[1, 2, 3], 0);
    let temp = tree(
        "holes",
        &[(
            LOW,
            sound_archive(&[
                FixtureMember::Bytes {
                    name: "ok.wav",
                    bytes: &good,
                },
                // Reaches past the member data into the index: in bounds of the
                // index, out of bounds of the container.
                FixtureMember::Extent {
                    name: "past_end.wav",
                    start: 0,
                    length: 1 << 20,
                },
                FixtureMember::Bytes {
                    name: "",
                    bytes: &good,
                },
            ]),
        )],
    );
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    assert_eq!(rows_of(&baseline, ContentKind::Sound).len(), 1);
    let status = status_of(&baseline, ContentKind::Sound);
    assert_eq!(status.rows, 1);
    assert_eq!(
        status.gaps.get("member_out_of_bounds"),
        Some(&1),
        "a member with no bytes inside the container cannot be a cue"
    );
    assert_eq!(status.gaps.get("member_name_empty"), Some(&1));
    assert_eq!(status.diagnostic, None);
}

/// A sound container whose own index does not read is a named gap, and the
/// other container still produces rows; when none can be read the collection
/// carries a diagnostic instead of silently holding nothing.
#[test]
fn accept_f14_d_7_a_refused_container_is_a_gap_and_does_not_take_the_other() {
    let good = pcm_member(11_025, &[1, 2, 3], 0);
    // The trailer version the pinned reader does not read for this game.
    let mut wrong_version = archive(&[("ok.wav", &good)]);
    let len = wrong_version.len();
    wrong_version[len - 8..len - 4].copy_from_slice(&7u32.to_le_bytes());
    let temp = tree(
        "refused",
        &[(LOW, wrong_version), (HIGH, archive(&[("ok.wav", &good)]))],
    );
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    assert_eq!(
        rows_of(&baseline, ContentKind::Sound)
            .iter()
            .map(|row| row.id.clone())
            .collect::<Vec<_>>(),
        vec![cue_id(HIGH, "ok.wav")],
        "the readable container still contributes"
    );
    let status = status_of(&baseline, ContentKind::Sound);
    assert_eq!(status.rows, 1);
    assert_eq!(
        status.gaps.get("unsupported_trailer_version"),
        Some(&1),
        "the refused container is counted under its own reader's code"
    );
    assert_eq!(status.diagnostic, None);
    // The rest of the inventory is unchanged.
    assert_eq!(baseline.roots, vec![cid(ContentKind::Mission, "ch1-m01")]);

    // A container whose bytes contradict the role rule is refused by dispatch,
    // never parsed as a sound container.
    let mut interp = vec![0u8; 64];
    interp[..4].copy_from_slice(&0x0897_1119u32.to_le_bytes());
    interp[4..8].copy_from_slice(&7u32.to_le_bytes());
    let refused = tree("contradiction", &[(LOW, interp)]);
    let baseline = retail_baseline(&refused.0).expect("the fixture installation reads");
    assert!(rows_of(&baseline, ContentKind::Sound).is_empty());
    let status = status_of(&baseline, ContentKind::Sound);
    assert_eq!(status.rows, 0);
    assert_eq!(status.gaps.get("header_role_conflict"), Some(&1));
    let diagnostic = status
        .diagnostic
        .as_deref()
        .expect("no readable container is a diagnostic, not a silent empty reading");
    assert!(diagnostic.contains("no "), "{diagnostic}");
    assert_eq!(baseline.roots, vec![cid(ContentKind::Mission, "ch1-m01")]);
}

/// An installation that inventories no sound container names the gap instead of
/// reporting an empty reading, and takes no other collection with it.
#[test]
fn accept_f14_d_7_a_missing_sound_container_is_a_named_gap_not_an_empty_reading() {
    let temp = tree("missing", &[]);
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    assert!(rows_of(&baseline, ContentKind::Sound).is_empty());
    let status = status_of(&baseline, ContentKind::Sound);
    assert_eq!(status.source, SOUND_CONTAINER_PATTERN);
    assert_eq!(status.rows, 0);
    let diagnostic = status
        .diagnostic
        .as_deref()
        .expect("a missing container is named, not dropped");
    assert!(diagnostic.contains(SOUND_CONTAINER_PATTERN), "{diagnostic}");
    assert!(diagnostic.contains("no "), "{diagnostic}");

    // The other collections and the denominator are untouched.
    assert_eq!(baseline.roots, vec![cid(ContentKind::Mission, "ch1-m01")]);
    assert_eq!(
        status_of(&baseline, ContentKind::Faction).rows,
        0,
        "the missing sound container does not disturb the other records"
    );
    let report = baseline_report_json(&baseline);
    assert!(!report.contains("\"sound\":"), "{report}");
    assert!(report.contains("\"kind\":\"sound\""), "{report}");
}

/// A sound is not launchable content: the collection adds no root, moves no
/// coverage denominator and stays visible as an unreachable unknown.
#[test]
fn accept_f14_d_7_the_collection_is_not_launchable_and_the_denominator_does_not_move() {
    let good = pcm_member(11_025, &[1, 2, 3], 0);
    let temp = tree(
        "roots",
        &[
            (LOW, archive(&[("ok.wav", &good)])),
            (HIGH, archive(&[("ok.wav", &good)])),
        ],
    );
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    assert!(!ContentKind::Sound.is_launchable());
    assert!(!ContentKind::Music.is_launchable());
    assert!(!ContentKind::Dialogue.is_launchable());
    assert_eq!(baseline.roots, vec![cid(ContentKind::Mission, "ch1-m01")]);
    assert_eq!(baseline.catalog.launchable_count(), 1);
    assert_eq!(baseline.catalog.synthetic_launchable_count(), 0);
    assert_eq!(baseline.coverage.roots, 1);
    assert_eq!(baseline.coverage.unresolved_references, 0);
    assert_eq!(baseline.coverage.unreachable_by_kind.get("sound"), Some(&2));
    assert_eq!(baseline.coverage.ready, 0);
}

// --------------------------------------------------------------- retail ----

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

/// The cue counts the retail acceptance pins, measured on the owner's
/// installation and recorded in
/// `docs/findings/2026-10-03-f14-d-7-sound-cue-collection.md`.
///
/// `soundsl.zbd` declares 2520 members under 2476 verbatim names; two of those
/// names differ only in letter case (`VO_c4-RM-m3_Blacke_9.wav` and
/// `VO_c4-RM-m3_blacke_9.wav`, byte-identical), which the id grammar folds into
/// one identity, so the archive holds 2475 cues and 45 byte-identical repeats.
/// `soundsh.zbd` declares 2521 members under 2477 names with the same fold, so
/// it holds 2476 cues and 45 repeats.
const RETAIL_LOW_CUES: usize = 2475;
const RETAIL_HIGH_CUES: usize = 2476;
const RETAIL_REPEATS: usize = 90;

/// The retail half: every readable member of both sound containers becomes a
/// `sound` row located by its own bytes, `music` and `dialogue` stay empty with
/// a named reason, and the coverage denominator is unchanged.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f14_d_7_retail_sound_cues_are_rows() {
    let dir = game_dir();
    let baseline = retail_baseline(&dir).expect("the original installation reads");
    let catalog = &baseline.catalog;

    let rows = rows_of(&baseline, ContentKind::Sound);
    assert_eq!(
        rows.len(),
        RETAIL_LOW_CUES + RETAIL_HIGH_CUES,
        "every cue both sound containers declare is a row; a different count must be \
         re-measured and this test updated, never silently accepted"
    );

    // One identity per (container, declared name), and every row names the
    // member's own stored bytes.
    let mut low = 0usize;
    let mut high = 0usize;
    for row in &rows {
        assert!(
            row.origin.is_original(),
            "{} must be installation data",
            row.id
        );
        let span = row
            .origin
            .source()
            .expect("a sound row is located by a span");
        let member = span
            .member_key()
            .unwrap_or_else(|| panic!("{} must name its member", row.id));
        assert!(
            member.to_ascii_lowercase().ends_with(".wav"),
            "{}: a sound cue is a recording",
            row.id
        );
        assert!(span.length() > 0, "{}: a member has bytes", row.id);
        assert_eq!(span.install_sha256().to_hex(), baseline.install_sha256);
        assert_eq!(
            span.member_sha256().map(|digest| digest.to_hex()),
            Some(
                cs_assets::install::sha256(&read_member(
                    &dir,
                    span.container_path(),
                    span.offset(),
                    span.length()
                ))
                .to_hex()
            ),
            "{}: the span's digest is the member's own bytes",
            row.id
        );
        assert_eq!(row.parse_state, ParseState::Parsed);
        assert_eq!(row.normalize_state, NormalizeState::NotNormalized);
        assert_eq!(row.readiness, Readiness::Unavailable);
        assert!(!row.is_ready());
        assert_eq!(reasons(row), vec!["not_normalized", "unknown"]);
        assert_eq!(
            unknown_claim(row),
            Some("f14.d.7.baseline.sound_playback"),
            "{}: F41's declared playback metadata stays unknown",
            row.id
        );
        assert!(row.runtime_consumers.is_empty());
        assert_eq!(row.dependencies.len(), 1);
        assert_eq!(row.dependencies[0].kind.label(), "static");
        assert_eq!(
            row.dependencies[0].provenance.claim_id.as_str(),
            "f14.d.7.baseline.sound_member"
        );
        assert_eq!(
            row.dependencies[0].provenance.class,
            ClaimStatus::ObservedTool
        );
        // The edge target is the inventory row of the container the bytes live in.
        assert_eq!(
            row.dependencies[0].target,
            cid(
                ContentKind::InstallFile,
                &install_file_key(span.container_path())
            ),
            "{}: one static edge onto its container's inventory row",
            row.id
        );
        match span.container_path() {
            path if path.eq_ignore_ascii_case(LOW) => low += 1,
            path if path.eq_ignore_ascii_case(HIGH) => high += 1,
            other => panic!("{other} is not a sound container of this installation"),
        }
    }
    assert_eq!(low, RETAIL_LOW_CUES);
    assert_eq!(high, RETAIL_HIGH_CUES);

    let status = status_of(&baseline, ContentKind::Sound);
    assert_eq!(status.source, SOUND_CONTAINER_PATTERN);
    assert_eq!(status.language, None);
    assert_eq!(status.rows, rows.len());
    assert_eq!(
        status.gaps.get("duplicate_member"),
        Some(&RETAIL_REPEATS),
        "the byte-identical repeats the two archives declare are counted, not duplicated"
    );
    assert_eq!(
        status.gaps.get("ambiguous_member_name"),
        None,
        "no name in either container declares two different recordings"
    );
    assert_eq!(status.diagnostic, None);

    // `music` and `dialogue` hold no row: nothing in the bytes states a class.
    for kind in [ContentKind::Music, ContentKind::Dialogue] {
        assert!(
            rows_of(&baseline, kind).is_empty(),
            "{kind} must hold no row"
        );
        let status = status_of(&baseline, kind);
        assert_eq!(status.rows, 0);
        assert_eq!(status.source, SOUND_CONTAINER_PATTERN);
        assert!(status.diagnostic.is_some());
    }

    // The denominator did not move: a sound is not launchable content.
    assert!(!ContentKind::Sound.is_launchable());
    assert!(
        baseline
            .roots
            .iter()
            .all(|id| id.kind() != ContentKind::Sound),
        "the sound collection is not a closure root"
    );
    assert_eq!(catalog.launchable_count(), baseline.roots.len());
    assert_eq!(catalog.synthetic_launchable_count(), 0);
    assert!(!catalog.is_fully_ready() || baseline.roots.is_empty());

    let report = baseline_report_json(&baseline);
    assert!(
        report.contains(&format!("\"sound\":{}", RETAIL_LOW_CUES + RETAIL_HIGH_CUES)),
        "{report}"
    );
    assert!(report.contains(&format!("\"duplicate_member\":{RETAIL_REPEATS}")));
    assert!(!report.contains("\"music\":"), "{report}");
    assert!(
        !report.contains("\"origin\":\"synthetic_fixture\""),
        "{report}"
    );
}

/// Reads the exact bytes a retail row's span names, straight from the
/// read-only installation.
fn read_member(dir: &Path, container: &str, offset: u64, length: u64) -> Vec<u8> {
    use std::io::{Read as _, Seek as _};
    let mut file = fs::File::open(dir.join(container)).expect("the container opens");
    file.seek(std::io::SeekFrom::Start(offset))
        .expect("the member offset is seekable");
    let mut bytes = vec![0u8; usize::try_from(length).expect("fits")];
    file.read_exact(&mut bytes)
        .expect("the member's own bytes read");
    bytes
}
