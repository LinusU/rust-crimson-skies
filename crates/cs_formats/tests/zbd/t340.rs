//! Acceptance for task #340: the ZBD family header signatures and archive
//! names read from the pinned mech3ax v0.6.0 source (commit
//! `d3521a9721be731d365504568ddcd78e3f9846bb`, `docs/research/SOURCES.md`
//! S02/S06) and checked against the retail installation
//! (`docs/findings/2026-09-28-t340-zbd-family-headers-and-archive-names.md`).
//!
//! The `accept_t340_*` synthetic tests author every byte here: the header
//! words are the documented constants, nothing is copied from original data.
//! `accept_t340_retail_every_zbd_archive_dispatches_to_its_family` reads the
//! installation at `$CS_GAME_DIR` (never writes it) and fails loudly without
//! it. `evidence_report_t340_writes_the_acceptance_report` is the evidence
//! harness (`docs/contracts/CLI-EVIDENCE.md`), not an acceptance test; its
//! run sequence is in its own doc comment.

use std::collections::{BTreeMap, VecDeque};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_formats::ParseContext;
use cs_formats::zbd::{
    ANIMATION_SIGNATURE, ANIMATION_VERSION, DispatchBasis, GAMEZ_SIGNATURE, GAMEZ_VERSION,
    HeaderStatus, INTERP_SIGNATURE, INTERP_VERSION, MemberExtent, MemberTable, RoleStatus,
    ZbdDispatch, ZbdDispatchError, ZbdFamily, ZbdProbe, ZbdReaderId, dispatch, family_record,
    read_reader_archive, read_sound_archive,
};
use cs_types::evidence::{ClaimStatus, SourceSpan};
use cs_types::install::RelativePath;

/// Provenance label carried by every synthetic result and error.
const CONTAINER: &str = "synthetic/t340_header.zbd";

/// A 12-byte header: `signature`, `version`, then an authored zero word.
fn header(signature: u32, version: u32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(12);
    bytes.extend_from_slice(&signature.to_le_bytes());
    bytes.extend_from_slice(&version.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes
}

/// The documented texture header words (`crates/mech3ax-image/src/textures.rs`:
/// u32 0, u32 1), followed by an authored count.
fn texture_leading_words() -> Vec<u8> {
    header(0, 1)
}

/// The first bytes of an authored sound archive: a RIFF member at offset 0.
const SOUND_LEADING_BYTES: &[u8] = b"RIFF\x24\x00\x00\x00WAVE";

pub(super) fn path(spelling: &str) -> RelativePath {
    RelativePath::new(spelling).expect("fixture spellings are valid relative paths")
}

fn dispatch_at<'probe>(
    path: &'probe RelativePath,
    header: &'probe [u8],
) -> Result<ZbdDispatch<'probe>, ZbdDispatchError> {
    dispatch(ZbdProbe::new(CONTAINER, path, header))
}

// --- GameZ ------------------------------------------------------------------

#[test]
fn accept_t340_gamez_signature_validates_planes_and_world_gamez() {
    assert_eq!(GAMEZ_SIGNATURE, 0x0297_1222);
    assert_eq!(GAMEZ_VERSION, 42);
    let bytes = header(GAMEZ_SIGNATURE, GAMEZ_VERSION);
    for spelling in ["zbd/planes.zbd", "ZBD/C1/GAMEZ.ZBD"] {
        let at = path(spelling);
        let decided = dispatch_at(&at, &bytes).expect("the documented GameZ header dispatches");
        assert_eq!(decided.family(), ZbdFamily::GameZ, "{spelling}");
        assert_eq!(decided.reader(), ZbdReaderId::GameZ);
        assert_eq!(decided.basis(), DispatchBasis::HeaderAndRole);
        assert_eq!(
            decided.header_status(),
            HeaderStatus::Validated {
                signature: GAMEZ_SIGNATURE,
                version: GAMEZ_VERSION
            }
        );
    }

    // The role now demands the signature: authored non-GameZ bytes at
    // `planes.zbd` are refused, not routed on the name alone.
    let planes = path("zbd/planes.zbd");
    let error =
        dispatch_at(&planes, &[0x5A; 12]).expect_err("GameZ bytes must carry the signature");
    assert_eq!(error.code(), "header_mismatch");
    let error = dispatch_at(&planes, &GAMEZ_SIGNATURE.to_le_bytes()[..3])
        .expect_err("a probe too short for the GameZ rule is refused");
    assert_eq!(error.code(), "header_too_short");

    let rule = family_record(ZbdFamily::GameZ)
        .header_rule()
        .signature()
        .expect("GameZ has a signature rule");
    assert_eq!(rule.evidence(), ClaimStatus::Documented);
    assert!(rule.source().contains("mech3ax"), "{}", rule.source());
}

// --- Animation --------------------------------------------------------------

#[test]
fn accept_t340_animation_signature_validates_camera_and_mission_archives() {
    assert_eq!(ANIMATION_SIGNATURE, 0x0817_0616);
    assert_eq!(ANIMATION_VERSION, 53);
    let bytes = header(ANIMATION_SIGNATURE, ANIMATION_VERSION);
    for spelling in ["zbd/c1/cam_anim.zbd", "zbd/c2b/m05/MIS_ANIM.ZBD"] {
        let at = path(spelling);
        let decided = dispatch_at(&at, &bytes).expect("the observed animation header dispatches");
        assert_eq!(decided.family(), ZbdFamily::Animation, "{spelling}");
        assert_eq!(decided.reader(), ZbdReaderId::Animation);
        assert_eq!(decided.basis(), DispatchBasis::HeaderAndRole);
        assert_eq!(
            decided.header_status(),
            HeaderStatus::Validated {
                signature: ANIMATION_SIGNATURE,
                version: ANIMATION_VERSION
            }
        );
    }

    // The signature is documented, the Crimson Skies version only observed,
    // and the rule says so instead of claiming `documented`.
    let rule = family_record(ZbdFamily::Animation)
        .header_rule()
        .signature()
        .expect("animation has a signature rule");
    assert_eq!(rule.evidence(), ClaimStatus::ObservedTool);
    assert!(rule.source().contains("observed"), "{}", rule.source());
}

#[test]
fn accept_t340_other_game_versions_are_refused() {
    // The pinned source's other-game versions share the signatures; none of
    // them is the Crimson Skies layout, so each fails explicitly.
    let cases: [(&str, u32, u32, u32); 6] = [
        ("zbd/planes.zbd", GAMEZ_SIGNATURE, 15, GAMEZ_VERSION),
        ("zbd/c1/gamez.zbd", GAMEZ_SIGNATURE, 27, GAMEZ_VERSION),
        ("zbd/c1/gamez.zbd", GAMEZ_SIGNATURE, 41, GAMEZ_VERSION),
        (
            "zbd/c1/cam_anim.zbd",
            ANIMATION_SIGNATURE,
            28,
            ANIMATION_VERSION,
        ),
        (
            "zbd/c1/m02/mis_anim.zbd",
            ANIMATION_SIGNATURE,
            39,
            ANIMATION_VERSION,
        ),
        (
            "zbd/c1/cam_anim.zbd",
            ANIMATION_SIGNATURE,
            50,
            ANIMATION_VERSION,
        ),
    ];
    for (spelling, signature, version, supported_version) in cases {
        let at = path(spelling);
        let bytes = header(signature, version);
        let error = dispatch_at(&at, &bytes).expect_err("another game's version must not dispatch");
        match error {
            ZbdDispatchError::UnsupportedHeaderVersion {
                observed,
                supported,
                ..
            } => {
                assert_eq!(observed, version, "{spelling}");
                assert_eq!(supported, supported_version, "{spelling}");
            }
            other => panic!("{spelling}: expected unsupported_header_version, got {other:?}"),
        }
        // Without a role the documented signature still gates the version.
        let mystery = path("zbd/mystery.zbd");
        let error = dispatch_at(&mystery, &bytes).expect_err("header-only dispatch checks it too");
        assert_eq!(error.code(), "unsupported_header_version");
    }
}

// --- Conflicts between the new signatures and roles -------------------------

#[test]
fn accept_t340_documented_signatures_conflict_with_other_roles() {
    let gamez = header(GAMEZ_SIGNATURE, GAMEZ_VERSION);
    let animation = header(ANIMATION_SIGNATURE, ANIMATION_VERSION);
    let interp = header(INTERP_SIGNATURE, INTERP_VERSION);

    let cases: [(&str, &[u8], ZbdFamily, ZbdFamily, u32); 6] = [
        // A signature family's role with another signature family's header.
        (
            "zbd/c1/cam_anim.zbd",
            &gamez,
            ZbdFamily::GameZ,
            ZbdFamily::Animation,
            GAMEZ_SIGNATURE,
        ),
        (
            "zbd/planes.zbd",
            &animation,
            ZbdFamily::Animation,
            ZbdFamily::GameZ,
            ANIMATION_SIGNATURE,
        ),
        (
            "zbd/c1/gamez.zbd",
            &interp,
            ZbdFamily::Interp,
            ZbdFamily::GameZ,
            INTERP_SIGNATURE,
        ),
        // A role-only family with a documented header.
        (
            "zbd/c1/zrdr.zbd",
            &gamez,
            ZbdFamily::GameZ,
            ZbdFamily::Reader,
            GAMEZ_SIGNATURE,
        ),
        (
            "zbd/soundsl.zbd",
            &animation,
            ZbdFamily::Animation,
            ZbdFamily::Sound,
            ANIMATION_SIGNATURE,
        ),
        (
            "zbd/c1/rtexture2.zbd",
            &gamez,
            ZbdFamily::GameZ,
            ZbdFamily::Texture,
            GAMEZ_SIGNATURE,
        ),
    ];
    for (spelling, bytes, expected_header, expected_role, expected_signature) in cases {
        let at = path(spelling);
        let error = dispatch_at(&at, bytes).expect_err("contradicting keys must not dispatch");
        match error {
            ZbdDispatchError::HeaderRoleConflict {
                header_family,
                role_family,
                signature,
                ..
            } => {
                assert_eq!(header_family, expected_header, "{spelling}");
                assert_eq!(role_family, expected_role, "{spelling}");
                assert_eq!(signature, expected_signature, "{spelling}");
            }
            other => panic!("{spelling}: expected header_role_conflict, got {other:?}"),
        }
    }
}

#[test]
fn accept_t340_header_only_dispatch_reaches_the_new_families() {
    let gamez = header(GAMEZ_SIGNATURE, GAMEZ_VERSION);
    let animation = header(ANIMATION_SIGNATURE, ANIMATION_VERSION);
    for (spelling, bytes, family) in [
        ("docs/scene.zbd", &gamez, ZbdFamily::GameZ),
        ("zbd/mystery.zbd", &animation, ZbdFamily::Animation),
    ] {
        let at = path(spelling);
        let decided = dispatch_at(&at, bytes).expect("a documented signature decides alone");
        assert_eq!(decided.family(), family, "{spelling}");
        assert_eq!(decided.basis(), DispatchBasis::HeaderOnly);
        assert!(matches!(
            decided.role_status(),
            RoleStatus::Unrecognized { .. }
        ));
    }

    // The texture header's constant words are *not* a signature: without a
    // role they identify nothing.
    let mystery = path("zbd/mystery.zbd");
    let error = dispatch_at(&mystery, &texture_leading_words())
        .expect_err("`0, 1` is not a texture signature");
    assert_eq!(error.code(), "unknown_family");
}

// --- Sound, reader and image archive names ----------------------------------

#[test]
fn accept_t340_sound_archives_route_to_the_sound_reader() {
    for spelling in ["zbd/soundsl.zbd", "ZBD/SOUNDSH.ZBD"] {
        let at = path(spelling);
        let decided = dispatch_at(&at, SOUND_LEADING_BYTES)
            .expect("`ZBD/sounds*.zbd` names the sound family");
        assert_eq!(decided.family(), ZbdFamily::Sound, "{spelling}");
        assert_eq!(decided.reader(), ZbdReaderId::Sound);
        assert_eq!(decided.basis(), DispatchBasis::RoleOnly);
        let HeaderStatus::Unvalidated { reason } = decided.header_status() else {
            panic!("a sound archive has no header to validate")
        };
        assert!(reason.contains("trailer"), "{reason}");
        let RoleStatus::Observed { rule } = decided.role_status() else {
            panic!("{spelling} is an observed sound archive name")
        };
        assert_eq!(rule.evidence(), ClaimStatus::Documented);
    }

    // The name was observed directly under `ZBD/` only.
    for spelling in ["zbd/c1/soundsl.zbd", "zbd/c1/m02/soundsh.zbd"] {
        let at = path(spelling);
        let error = dispatch_at(&at, SOUND_LEADING_BYTES)
            .expect_err("a sound name at an unobserved level is no role evidence");
        assert_eq!(error.code(), "unknown_family", "{spelling}");
    }

    // The dispatched sound archive reaches the sound reader, and only it.
    let sound_path = path("zbd/soundsh.zbd");
    let decided = dispatch_at(&sound_path, SOUND_LEADING_BYTES).expect("dispatches");
    let bytes = [SOUND_LEADING_BYTES, &[0u8; 4][..]].concat();
    let members = [MemberExtent::new(
        b"gun.wav",
        None,
        SourceSpan {
            offset: 0,
            length: bytes.len() as u64,
        },
    )];
    let table = MemberTable::from_dispatch(&decided, &members);
    let mut context = ParseContext::with_defaults(CONTAINER);
    let archive = read_sound_archive(&mut context, &table, &bytes)
        .expect("the sound reader reads a dispatched sound archive");
    assert_eq!(archive.len(), 1);
    assert_eq!(
        archive.entry(0).expect("readable").content(),
        bytes.as_slice()
    );
    let error = read_reader_archive(&mut context, &table, &bytes)
        .expect_err("the reader reader refuses sound bytes");
    assert_eq!(error.code(), "family_mismatch");
}

#[test]
fn accept_t340_content_root_reader_and_image_archives() {
    // `ZBD/zrdr.zbd` sits directly under the content root as well.
    let root_reader = path("zbd/zrdr.zbd");
    let reader_bytes = header(4, 2);
    let decided = dispatch_at(&root_reader, &reader_bytes).expect("root `zrdr.zbd` dispatches");
    assert_eq!(decided.family(), ZbdFamily::Reader);
    assert_eq!(decided.basis(), DispatchBasis::RoleOnly);

    // `ZBD/rimage.zbd` is an image package of the texture family.
    let rimage = path("ZBD/rimage.zbd");
    let texture_bytes = texture_leading_words();
    let decided = dispatch_at(&rimage, &texture_bytes).expect("`rimage.zbd` dispatches");
    assert_eq!(decided.family(), ZbdFamily::Texture);
    assert_eq!(decided.basis(), DispatchBasis::RoleOnly);
    let HeaderStatus::Unvalidated { reason } = decided.header_status() else {
        panic!("texture archives have no header signature")
    };
    assert!(reason.contains("no signature"), "{reason}");

    // Every role rule is now documented by the pinned source's README or an
    // earlier citation — none is inferred from a name alone.
    for family in ZbdFamily::ALL {
        for rule in family_record(family).role_rules() {
            assert_eq!(
                rule.evidence(),
                ClaimStatus::Documented,
                "{family:?}: {}",
                rule.pattern()
            );
        }
    }
}

// --- Retail -----------------------------------------------------------------

/// Size of one member-table entry of a version-one archive
/// (`EntryC` in `crates/mech3ax-archive/src/archive.rs`: u32 start, u32
/// length, 64-byte name, 76 bytes).
const ARCHIVE_ENTRY_BYTES: u64 = 148;

/// One retail `.zbd` archive and what production dispatch decided for it.
#[derive(Debug)]
struct RetailArchive {
    spelling: String,
    size_bytes: u64,
    family: ZbdFamily,
    basis: DispatchBasis,
    header_status: HeaderStatus,
    /// `(version, member count)` of the version-one trailer, for the
    /// families indexed at the end.
    trailer: Option<(u32, u32)>,
}

/// Every `.zbd` file under `game_dir`, sorted by spelling.
pub(super) fn retail_zbd_files(game_dir: &Path) -> Vec<(String, PathBuf)> {
    let mut found = Vec::new();
    let mut pending = vec![game_dir.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("read {}: {error}", directory.display()));
        for entry in entries {
            let entry = entry.expect("directory entry");
            let file_type = entry.file_type().expect("file type");
            let host = entry.path();
            if file_type.is_dir() {
                pending.push(host);
            } else if file_type.is_file()
                && host
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("zbd"))
            {
                let relative = host.strip_prefix(game_dir).expect("under the game dir");
                let spelling = relative
                    .components()
                    .map(|component| component.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                found.push((spelling, host));
            }
        }
    }
    found.sort();
    found
}

pub(super) fn read_at(file: &mut File, offset: u64, length: usize) -> Vec<u8> {
    file.seek(SeekFrom::Start(offset)).expect("seek");
    let mut bytes = vec![0; length];
    file.read_exact(&mut bytes).expect("read");
    bytes
}

pub(super) fn le_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"))
}

/// Reads the version-one trailer and table the pinned source documents and
/// checks every member extent against it; returns `(version, count)` and
/// the member extents `(start, length)`.
fn version_one_trailer(file: &mut File, size: u64, spelling: &str) -> (u32, u32, Vec<(u64, u64)>) {
    assert!(size >= 8, "{spelling}: too short for a trailer");
    let tail = read_at(file, size - 8, 8);
    let (version, count) = (le_u32(&tail, 0), le_u32(&tail, 4));
    assert_eq!(version, 1, "{spelling}: trailer version");
    let table_bytes = u64::from(count) * ARCHIVE_ENTRY_BYTES;
    assert!(table_bytes + 8 <= size, "{spelling}: the member table fits");
    let table_start = size - 8 - table_bytes;
    let table = read_at(file, table_start, table_bytes as usize);
    let mut extents = Vec::with_capacity(count as usize);
    for index in 0..count as usize {
        let row = &table[index * ARCHIVE_ENTRY_BYTES as usize..];
        let (start, length) = (u64::from(le_u32(row, 0)), u64::from(le_u32(row, 4)));
        assert!(
            start < start + length && start + length <= table_start,
            "{spelling}: member {index} lies before the table"
        );
        extents.push((start, length));
    }
    (version, count, extents)
}

/// Dispatches every retail `.zbd` archive with production code and checks
/// the layout facts the findings record.
fn retail_archives(game_dir: &Path) -> Vec<RetailArchive> {
    let mut archives = Vec::new();
    for (spelling, host) in retail_zbd_files(game_dir) {
        let mut file = File::open(&host).unwrap_or_else(|error| panic!("open {spelling}: {error}"));
        let size = file.metadata().expect("metadata").len();
        let probe = read_at(&mut file, 0, size.min(64) as usize);
        let relative = path(&spelling);
        let decided = dispatch(ZbdProbe::new(&spelling, &relative, &probe))
            .unwrap_or_else(|error| panic!("{spelling} must dispatch: {error}"));
        let family = decided.family();
        let mut trailer = None;
        match family {
            ZbdFamily::Interp | ZbdFamily::GameZ | ZbdFamily::Animation => {
                assert_eq!(decided.basis(), DispatchBasis::HeaderAndRole, "{spelling}");
                assert!(
                    matches!(decided.header_status(), HeaderStatus::Validated { .. }),
                    "{spelling}"
                );
            }
            ZbdFamily::Texture => {
                assert_eq!(decided.basis(), DispatchBasis::RoleOnly, "{spelling}");
                // The documented texture header words.
                assert_eq!((le_u32(&probe, 0), le_u32(&probe, 4)), (0, 1), "{spelling}");
            }
            ZbdFamily::Sound | ZbdFamily::Reader => {
                assert_eq!(decided.basis(), DispatchBasis::RoleOnly, "{spelling}");
                let (version, count, extents) = version_one_trailer(&mut file, size, &spelling);
                assert!(count > 0, "{spelling}: members");
                if family == ZbdFamily::Sound {
                    for (start, length) in extents {
                        assert!(length >= 12, "{spelling}: member at {start}");
                        let riff = read_at(&mut file, start, 12);
                        assert_eq!(&riff[0..4], b"RIFF", "{spelling}: member at {start}");
                        assert_eq!(&riff[8..12], b"WAVE", "{spelling}: member at {start}");
                    }
                }
                trailer = Some((version, count));
            }
        }
        let (basis, header_status) = (decided.basis(), decided.header_status());
        archives.push(RetailArchive {
            spelling,
            size_bytes: size,
            family,
            basis,
            header_status,
            trailer,
        });
    }
    archives
}

pub(super) fn game_dir() -> PathBuf {
    let dir = std::env::var_os("CS_GAME_DIR")
        .expect("CS_GAME_DIR is not set: this test needs the original installation");
    let dir = PathBuf::from(dir);
    assert!(
        dir.is_dir(),
        "CS_GAME_DIR is not a directory: {}",
        dir.display()
    );
    dir
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t340_retail_every_zbd_archive_dispatches_to_its_family() {
    let archives = retail_archives(&game_dir());
    assert!(
        !archives.is_empty(),
        "no `.zbd` archive found under CS_GAME_DIR"
    );

    let mut per_family: BTreeMap<&str, usize> = BTreeMap::new();
    for archive in &archives {
        *per_family.entry(archive.family.as_str()).or_default() += 1;
    }
    // Every family is reached, including sound, which F06-A could not route.
    for family in ZbdFamily::ALL {
        assert!(
            per_family
                .get(family.as_str())
                .is_some_and(|count| *count > 0),
            "no retail archive dispatched to {family:?}: {per_family:?}"
        );
    }
    let sound: Vec<&str> = archives
        .iter()
        .filter(|archive| archive.family == ZbdFamily::Sound)
        .map(|archive| archive.spelling.as_str())
        .collect();
    assert!(
        sound
            .iter()
            .all(|spelling| spelling.to_ascii_lowercase().starts_with("zbd/sounds")),
        "{sound:?}"
    );
    println!("retail .zbd archives per family: {per_family:?}");
}

// --- Evidence harness ---------------------------------------------------------

/// Evidence-report harness for task #340 (`docs/contracts/CLI-EVIDENCE.md`,
/// schema `schemas/evidence.schema.json`). Not an acceptance test: it fails
/// loudly when its inputs are missing. Run from the workspace root:
///
/// 1. ```sh
///    mkdir -p private/evidence/T340
///    cargo test --workspace --locked -- accept_t340_ --include-ignored \
///      2>&1 | tee private/evidence/T340/cargo-test.log
///    ```
///    (record the exit status of `cargo test`, e.g. `${pipestatus[1]}` in zsh.)
/// 2. ```sh
///    CS_EVIDENCE_DIR=private/evidence/T340 \
///    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
///    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_t340_ --include-ignored" \
///    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
///      cargo test --locked -p cs_formats --test zbd -- evidence_report_t340 --ignored
///    ```
/// 3. ```sh
///    python3 tools/validate_evidence.py private/evidence/T340/acceptance.json \
///      --artifact-root private/evidence/T340 --require-pass
///    ```
/// 4. Commit a copy of `acceptance.json` as `docs/findings/evidence/T340.json`.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_t340_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = game_dir();

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be the tree of the tested commit"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", log_path.display()));
    let suite = parse_suite(&log, "accept_t340_");
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_t340_` tests were recorded in {}",
        log_path.display()
    );
    assert!(
        suite.assertions.len() as u64 >= suite.passed,
        "fewer per-test results than passing tests were parsed; inspect the log"
    );
    let retail = "accept_t340_retail_every_zbd_archive_dispatches_to_its_family";
    let status = suite
        .assertions
        .iter()
        .find(|(name, _)| short_name(name) == retail)
        .map(|(_, status)| *status)
        .unwrap_or_else(|| panic!("{retail} did not run: run step 1 with --include-ignored"));
    assert_eq!(status, "pass", "{retail} must pass");

    // Installation hashes from production discovery (F02).
    let found = cs_assets::install::discover(&game_dir)
        .expect("production discovery reads the original installation");
    let install_sha256 = cs_assets::install::fingerprint(&found.manifest).to_hex();
    let content_sha256 = cs_assets::install::content_fingerprint(&found.manifest).to_hex();

    // The dispatch table of every retail archive: spellings, sizes, families
    // and header words — no original bytes.
    let archives = retail_archives(&game_dir);
    let hashes: BTreeMap<String, String> = found
        .manifest
        .files
        .iter()
        .map(|record| {
            (
                record.relative_spelling.as_str().to_owned(),
                record.sha256.to_hex(),
            )
        })
        .collect();
    let dispatch_path = evidence_dir.join("zbd-dispatch.json");
    fs::write(
        &dispatch_path,
        dispatch_json(&candidate_tree, &install_sha256, &archives, &hashes),
    )
    .unwrap_or_else(|error| panic!("write {}: {error}", dispatch_path.display()));

    let artifacts = [artifact(&log_path, "log"), artifact(&dispatch_path, "json")];
    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"T340\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {{\"rust\": {}, \"bevy\": {}, \"avian\": {}}},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": [{}], \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        jstr(&command_output("rustc", &["--version"])),
        jstr(&locked_version("bevy")),
        jstr(&locked_version("avian3d")),
        jstr(&iso_utc_now()),
        argv.iter()
            .map(|arg| jstr(arg))
            .collect::<Vec<_>>()
            .join(", "),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.passed + suite.failed + suite.ignored,
        suite.passed + suite.failed,
        suite.passed,
        suite.failed,
        suite.ignored,
        suite
            .assertions
            .iter()
            .map(|(name, status)| format!(
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\"]}}",
                jstr(short_name(name))
            ))
            .collect::<Vec<_>>()
            .join(", "),
        artifacts
            .iter()
            .map(|(name, digest, kind)| format!(
                "{{\"path\": {}, \"sha256\": {digest:?}, \"kind\": {kind:?}}}",
                jstr(name)
            ))
            .collect::<Vec<_>>()
            .join(", "),
        jstr(
            "implementer: claude-1/claude-1 (Rally #340, implement claim of \
            2026-09-28T16:48:36Z, handed over at 17:07:42Z); reviewer: claude-1/claude-1 again, \
            on the review claim of 2026-09-28T17:08:00Z, which merged it at 17:09:37Z. The same \
            agent instance is on both sides, so this review is not independent and is not \
            independent original-reference evidence; the review claim started eighteen seconds \
            after the hand-over, so the activity log cannot prove a fresh context and none is \
            claimed. No agent review replaces the owner's human approval"
        ),
        jstr(
            "acceptance suite run locally with the retail capability; this harness derives \
             every field from the recorded log, production discovery of $CS_GAME_DIR, the \
             production dispatch of every retail .zbd archive (zbd-dispatch.json), rustc and \
             Cargo.lock; validated with tools/validate_evidence.py --require-pass"
        ),
    );
    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must not validate",
        suite.failed
    );
    println!("wrote {}", out.display());
}

fn dispatch_json(
    candidate_tree: &str,
    install_sha256: &str,
    archives: &[RetailArchive],
    hashes: &BTreeMap<String, String>,
) -> String {
    let rows: Vec<String> = archives
        .iter()
        .map(|archive| {
            let header = match archive.header_status {
                HeaderStatus::Validated { signature, version } => format!(
                    "{{\"validated\": true, \"signature\": \"0x{signature:08X}\", \"version\": {version}}}"
                ),
                HeaderStatus::Unvalidated { .. } => "{\"validated\": false}".to_owned(),
            };
            let trailer = archive.trailer.map_or_else(
                || "null".to_owned(),
                |(version, count)| format!("{{\"version\": {version}, \"members\": {count}}}"),
            );
            let sha = hashes
                .get(&archive.spelling)
                .map_or_else(|| "null".to_owned(), |hash| jstr(hash));
            format!(
                "{{\"spelling\": {}, \"size_bytes\": {}, \"sha256\": {sha}, \"family\": {}, \
                 \"basis\": {}, \"header\": {header}, \"trailer\": {trailer}}}",
                jstr(&archive.spelling),
                archive.size_bytes,
                jstr(archive.family.as_str()),
                jstr(&format!("{:?}", archive.basis)),
            )
        })
        .collect();
    format!(
        "{{\n \"task_id\": \"T340\",\n \"candidate_tree\": {},\n \"install_sha256\": {},\n \
         \"archive_count\": {},\n \"archives\": [\n  {}\n ]\n}}\n",
        jstr(candidate_tree),
        jstr(install_sha256),
        archives.len(),
        rows.join(",\n  ")
    )
}

// ------------------------------------------------------ harness helpers ---

pub(super) fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!("{name} is not set: run the sequence in the evidence harness doc comment")
    })
}

/// Cargo runs tests from the package root; re-anchor a workspace-relative path.
pub(super) fn workspace_path(as_described: &str) -> PathBuf {
    let path = PathBuf::from(as_described);
    if path.is_absolute() {
        return path;
    }
    Path::new(&git(&["rev-parse", "--show-toplevel"])).join(path)
}

pub(super) fn command_output(program: &str, args: &[&str]) -> String {
    let output = Command::new(program)
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("{program} runs: {error}"));
    assert!(output.status.success(), "{program} {args:?} failed");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

pub(super) fn git(args: &[&str]) -> String {
    command_output("git", args)
}

/// The locked version of one `Cargo.lock` package.
pub(super) fn locked_version(package: &str) -> String {
    let lock_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
    let lock = fs::read_to_string(&lock_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", lock_path.display()));
    let mut wanted = false;
    for line in lock.lines().map(str::trim) {
        if line == "[[package]]" {
            wanted = false;
        } else if let Some(name) = line.strip_prefix("name = \"") {
            wanted = name.trim_end_matches('"') == package;
        } else if let Some(version) = line.strip_prefix("version = \"")
            && wanted
        {
            return version.trim_end_matches('"').to_owned();
        }
    }
    panic!("package {package:?} is not in {}", lock_path.display());
}

/// A test name without its module path.
pub(super) fn short_name(name: &str) -> &str {
    name.rsplit("::").next().unwrap_or(name)
}

#[derive(Debug, Default)]
pub(super) struct Suite {
    pub(super) passed: u64,
    pub(super) failed: u64,
    pub(super) ignored: u64,
    /// `(test name, "pass" | "fail")`, in log order, deduplicated.
    pub(super) assertions: Vec<(String, &'static str)>,
}

/// The libtest summaries plus the per-test results of the tests whose name
/// starts with `prefix` in a recorded `cargo test` output.
pub(super) fn parse_suite(log: &str, prefix: &str) -> Suite {
    let mut suite = Suite::default();
    let mut pending: VecDeque<String> = VecDeque::new();
    for line in log.lines() {
        let trimmed = line.trim_start();
        if let Some(summary) = trimmed.strip_prefix("test result:") {
            for segment in summary.split(';') {
                let words: Vec<&str> = segment.split_whitespace().collect();
                // The first segment is `ok. 8 passed`: find the number word.
                if let Some(pair) = words.windows(2).find(|pair| pair[0].parse::<u64>().is_ok()) {
                    let count: u64 = pair[0].parse().expect("checked");
                    match pair[1] {
                        "passed" => suite.passed += count,
                        "failed" => suite.failed += count,
                        "ignored" => suite.ignored += count,
                        _ => {}
                    }
                }
            }
            continue;
        }
        if !pending.is_empty() && (trimmed == "ok" || trimmed == "FAILED") {
            let name = pending.pop_front().expect("pending test");
            record(
                &mut suite,
                name,
                if trimmed == "ok" { "pass" } else { "fail" },
            );
            continue;
        }
        let mut cursor = trimmed;
        while let Some(position) = cursor.find("test ") {
            let after = &cursor[position + 5..];
            let Some(separator) = after.find(" ... ") else {
                break;
            };
            let name = after[..separator].to_owned();
            let tail = &after[separator + 5..];
            cursor = tail;
            if !short_name(&name).starts_with(prefix) {
                continue;
            }
            match tail.split_whitespace().next() {
                Some("ok") => record(&mut suite, name, "pass"),
                Some("FAILED") => record(&mut suite, name, "fail"),
                _ => pending.push_back(name),
            }
        }
    }
    suite
}

fn record(suite: &mut Suite, name: String, status: &'static str) {
    if !suite.assertions.iter().any(|(seen, _)| *seen == name) {
        suite.assertions.push((name, status));
    }
}

/// `(file name, sha256, kind)` of one artifact inside the evidence directory.
pub(super) fn artifact(path: &Path, kind: &str) -> (String, String, String) {
    let bytes = fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let name = path
        .file_name()
        .expect("artifact file name")
        .to_string_lossy()
        .into_owned();
    (
        name,
        cs_assets::install::sha256(&bytes).to_hex(),
        kind.to_owned(),
    )
}

/// A JSON string literal.
pub(super) fn jstr(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if (control as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// RFC 3339 UTC with whole seconds.
pub(super) fn iso_utc_now() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after 1970")
        .as_secs() as i64;
    // Howard Hinnant's `civil_from_days`.
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        (rest % 3_600) / 60,
        rest % 60
    )
}
