//! Shared retail census for the T582 acceptance test and its evidence
//! harness (Rally task #582, "Measure the legacy custom-aircraft layout
//! before a custom-plane catalog collection").
//!
//! It is a `tests/` subdirectory module, not a test target of its own: it has
//! no `#[test]`, and cargo never discovers `tests/<dir>/mod.rs` as a binary.
//! Both `accept_t582_legacy_custom_aircraft_census.rs` and
//! `evidence_report_t582.rs` include it with
//! `#[path = "t582_support/mod.rs"]`, so the acceptance suite and the
//! evidence artifact are two readings of the same installation through
//! **one** census — a second census would be able to disagree with the
//! assertions.
//!
//! # What is measured
//!
//! F64-B checked the installation's 228 inventoried **file spellings**
//! against the storage shapes the engine image itself names and found zero
//! matches. What that sweep could not see is what ships *inside* a
//! container. This census closes that gap with production readers only:
//!
//! * every `.rof` file is parsed with `cs_formats::rof::read_tree` and every
//!   member is decoded with `cs_formats::rof::read_member`;
//! * every file under `zbd/` is dispatched by `cs_formats::zbd::dispatch`
//!   (role rule + documented header signature), and every trailer-indexed
//!   container (sound, reader) is listed with
//!   `cs_formats::zbd::read_version_one_index` + `cs_formats::zbd::list_members`;
//! * everything else is recorded as a non-container file;
//! * every inventoried file lands in exactly one class, so `unclassified`
//!   must stay empty;
//! * every member name (and every file spelling) is swept against the
//!   **same** storage shapes F64-B measured in the engine image — a stored
//!   plane or save hidden inside a container would match one. Every hit is
//!   recorded as a candidate with its byte evidence; the acceptance asserts
//!   that none is one the engine's own path template could open.
//!
//! No format is ever inferred from a file name: the class of a container is
//! what the production readers parsed it as, and a file no reader claims is
//! counted as a non-container, never typed by its extension.
//!
//! **Never returned, printed or committed:** any original file content.
//! Only spellings, counts, families, extents and digests leave this module.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use cs_assets::install::{self, content_fingerprint, fingerprint};
use cs_formats::ParseContext;
use cs_formats::rof::{RofLimits, read_member, read_tree};
use cs_formats::zbd::{
    ZbdFamily, ZbdProbe, dispatch, indexed_by_trailer, list_members, read_version_one_index,
    role_for_path,
};
use cs_types::install::InstallManifest;

/// The member measured (by F64-B) to reference stored custom planes. The
/// census re-reads it through the production ROF mount so the reference and
/// the container census are two facts about the same bytes.
pub const PLANE_CONSTRUCTION: &str = "ASSETS/SCRIPTS/PLANECONSTRUCTION.SCRIPT";

/// The container that member lives in.
pub const BASE_CONTAINER: &str = "GOSDATA/ASSETS/crimson.rof";

/// How many leading bytes of a ZBD candidate the census hands to
/// `dispatch`. Enough for every documented signature offset (F06's probe
/// rules use at most the first 16 bytes).
const ZBD_PROBE_BYTES: usize = 64;

/// The storage shapes the engine image's own path templates name (the same
/// six F64-B measured, `docs/findings/2026-10-07-f64-b-verified-read-only-import-subset.md`).
/// A legacy save or custom-plane file **is** one of these shapes; the census
/// sweeps them over file spellings *and* member names, because a stored
/// plane could in principle be shipped as a container member.
pub const ABSENT_SHAPES: [&str; 6] = [
    "SavedGames",
    "Planes\\",
    ".sav",
    "Status.dat",
    "Mission.",
    "Persist.",
];

/// One storage-shape hit, with the byte evidence that locates it.
///
/// A hit is a **candidate, never a verdict**: the sweep is a substring
/// sweep, so every hit is recorded with where it was found and whether the
/// spelling is one the engine's measured template could actually *open*
/// (`template_faithful`). A hit that is not template-faithful — say the
/// substring `mission.` inside a graphics member's name — stays a recorded
/// candidate, examined and listed in the findings note, and is never
/// silently dropped and never treated as a shipped save.
#[derive(Debug)]
pub struct ShapeCandidate {
    /// Where the hit was found (`member <container>#<name>`, `file <spelling>`).
    pub location: String,
    /// The shape that matched.
    pub shape: &'static str,
    /// Whether the spelling is one the engine's own template could open:
    /// `Planes\`/`SavedGames` as a path component, `.sav` as the file
    /// ending, `Status.dat` as the whole file name, and `Mission.`/
    /// `Persist.` only before the digits the `%1d%02d` templates spell.
    pub template_faithful: bool,
    /// The stored extent's offset inside its container, when it has one.
    pub offset: Option<u64>,
    /// The stored extent's length, when measured.
    pub length: Option<u64>,
}

/// One `.rof` container as the production tree reader parsed it.
#[derive(Debug)]
pub struct RofCensus {
    /// The container's inventoried relative spelling.
    pub spelling: String,
    /// Members the tree declared.
    pub members: usize,
    /// Members the production member reader decoded.
    pub decoded_ok: usize,
    /// Members the production member reader refused, as `name: code`.
    pub decode_failures: Vec<String>,
    /// Every member's spelling (`/`-joined), in tree order.
    pub member_spellings: Vec<String>,
}

/// The installation-wide container census, through production readers only.
#[derive(Debug)]
pub struct Census {
    /// The installation digest this census is bound to.
    pub install_sha256: String,
    /// The canonical-content digest of the same discovery.
    pub content_sha256: String,
    /// Inventoried files, exactly as production discovery lists them.
    pub file_count: usize,
    /// Every `.rof` container, parsed and fully decoded.
    pub rof_containers: Vec<RofCensus>,
    /// Dispatched ZBD files per family, by the family's own label.
    pub zbd_files_by_family: BTreeMap<&'static str, usize>,
    /// Trailer-indexed containers (sound, reader) that listed successfully.
    pub zbd_indexed_containers: usize,
    /// Members those containers declared.
    pub zbd_indexed_members: usize,
    /// Members that were not readable, as `container/member: error`.
    pub zbd_member_failures: Vec<String>,
    /// Container spellings whose trailer index could not be read.
    pub zbd_index_failures: Vec<String>,
    /// Files outside the container roots that no container reader claims.
    pub plain_files: Vec<String>,
    /// Files no production reader could classify (a refused container).
    pub unclassified: Vec<String>,
    /// Every storage-shape hit in a spelling or member name, with its byte
    /// evidence. The retail acceptance asserts no hit is template-faithful;
    /// the substring hits are recorded candidates, examined in the findings
    /// note, never silently dropped.
    pub stored_plane_candidates: Vec<ShapeCandidate>,
}

/// The read-only installation root, or a loud failure: a retail test must
/// fail, not pass, when `CS_GAME_DIR` is absent.
pub fn game_dir() -> PathBuf {
    let dir = std::env::var_os("CS_GAME_DIR").expect(
        "CS_GAME_DIR is not set: this census needs the original installation \
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

/// The installation's manifest, discovered once per process.
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

/// Where `name` matches one of the engine's storage shapes, with the
/// original engine's case-insensitive filesystem and both separator
/// spellings — the sweep F64-B's acceptance performs on file spellings,
/// extended here to member names — and whether the match is one the
/// engine's own measured template could open.
///
/// The template-faithful tier follows the templates byte for byte: a
/// directory component for `Planes\*.*` and `SavedGames\%s\...`, the file
/// ending for `%s\%s.sav`, the whole file name for `%s\Status.dat`, and
/// `%s\Mission.%1d%02d` / `%s\Persist.%1d%02d` only where the dot is
/// followed by the digits the format strings spell. A substring that the
/// no template could address (a graphics member whose name contains
/// `FLYMISSION.`) is still a hit, but not a faithful one.
pub fn shape_candidate(name: &str) -> Option<(&'static str, bool)> {
    let lowered = name.to_lowercase();
    // The last path component, for the whole-name / file-ending tiers.
    let last = lowered
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(&lowered)
        .to_owned();
    for shape in ABSENT_SHAPES {
        let native = shape.to_lowercase();
        let forward = shape.replace('\\', "/").to_lowercase();
        let substring = lowered.contains(&native) || lowered.contains(&forward);
        if !substring {
            continue;
        }
        let faithful = match shape {
            // A directory component the engine's path templates spell.
            "SavedGames" | "Planes\\" => {
                let component = native.trim_end_matches('\\');
                last == component
                    || lowered.contains(&format!("/{component}/"))
                    || lowered.contains(&format!("\\{component}\\"))
                    || lowered.contains(&format!("/{component}\\"))
                    || lowered.contains(&format!("\\{component}/"))
            }
            // `%s\%s.sav`: the file itself ends in `.sav`.
            ".sav" => last.ends_with(".sav"),
            // `%s\Status.dat`: the file itself is named `Status.dat`.
            "Status.dat" => last == "status.dat",
            // `%s\Mission.%1d%02d` / `%s\Persist.%1d%02d`: the dot is
            // followed by the digits the format string spells.
            "Mission." | "Persist." => {
                let stem = native.trim_end_matches('.');
                match last.strip_prefix(&format!("{stem}.")) {
                    Some(rest) => rest.starts_with(|ch: char| ch.is_ascii_digit()),
                    None => false,
                }
            }
            _ => false,
        };
        return Some((shape, faithful));
    }
    None
}

/// The full installation census, through production readers only.
///
/// Every inventoried file lands in exactly one class; a container the
/// production readers refuse is `unclassified`, never assumed.
pub fn census(root: &Path) -> Census {
    let manifest = installation(root);
    let mut out = Census {
        install_sha256: fingerprint(&manifest).to_hex(),
        content_sha256: content_fingerprint(&manifest).to_hex(),
        file_count: manifest.files.len(),
        rof_containers: Vec::new(),
        zbd_files_by_family: BTreeMap::new(),
        zbd_indexed_containers: 0,
        zbd_indexed_members: 0,
        zbd_member_failures: Vec::new(),
        zbd_index_failures: Vec::new(),
        plain_files: Vec::new(),
        unclassified: Vec::new(),
        stored_plane_candidates: Vec::new(),
    };

    for record in &manifest.files {
        let spelling = record.relative_spelling.as_str();
        let logical = record.relative_spelling.logical_key();
        let path = root.join(spelling);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                out.unclassified
                    .push(format!("{spelling}: cannot read file: {error}"));
                continue;
            }
        };

        if logical.ends_with(".rof") {
            census_rof(&mut out, spelling, &bytes);
        } else if logical.starts_with("zbd/") {
            census_zbd(&mut out, spelling, record.relative_spelling.clone(), &bytes);
        } else {
            // A file outside the container roots: no container reader
            // claims it, and its extension is never used to type it.
            if let Some((shape, faithful)) = shape_candidate(spelling) {
                out.stored_plane_candidates.push(ShapeCandidate {
                    location: format!("file {spelling}"),
                    shape,
                    template_faithful: faithful,
                    offset: None,
                    length: Some(record.size_bytes),
                });
            }
            out.plain_files.push(spelling.to_owned());
        }
    }
    out
}

/// One `.rof` container: the tree is parsed and **every** member is decoded
/// through the production member reader, so "accounted for" means the
/// reader handed out the bytes, not that a name was listed.
fn census_rof(out: &mut Census, spelling: &str, bytes: &[u8]) {
    let mut context = ParseContext::with_defaults(spelling);
    let tree = match read_tree(&mut context, bytes) {
        Ok(tree) => tree,
        Err(error) => {
            out.unclassified
                .push(format!("{spelling}: rof tree refused: {}", error.code()));
            return;
        }
    };
    let mut member_spellings = Vec::with_capacity(tree.members().len());
    let mut decode_failures = Vec::new();
    let mut decoded_ok = 0;
    for member in tree.members() {
        let name = join_member_path(&member.path);
        if let Some((shape, faithful)) = shape_candidate(&name) {
            out.stored_plane_candidates.push(ShapeCandidate {
                location: format!("member {spelling}#{name}"),
                shape,
                template_faithful: faithful,
                offset: Some(member.start),
                length: Some(member.stored_len()),
            });
        }
        match read_member(&context, bytes, member, &RofLimits::default()) {
            Ok(_) => decoded_ok += 1,
            Err(error) => decode_failures.push(format!("{name}: {}", error.code())),
        }
        member_spellings.push(name);
    }
    out.rof_containers.push(RofCensus {
        spelling: spelling.to_owned(),
        members: tree.members().len(),
        decoded_ok,
        decode_failures,
        member_spellings,
    });
}

/// One ZBD file: dispatched by the production role rule + header rule, and
/// listed through the trailer index when the family is indexed. A family
/// with no member index (texture, interp, gamez, animation) is accounted
/// as a classified container, never forced through a reader that does not
/// exist for it.
fn census_zbd(
    out: &mut Census,
    spelling: &str,
    relative: cs_types::install::RelativePath,
    bytes: &[u8],
) {
    let probe_len = bytes.len().min(ZBD_PROBE_BYTES);
    let decided = match dispatch(ZbdProbe::new(spelling, &relative, &bytes[..probe_len])) {
        Ok(decided) => decided,
        Err(error) => {
            let role = role_for_path(&relative);
            out.unclassified.push(format!(
                "{spelling}: zbd dispatch refused: {} (role {role:?})",
                error.code()
            ));
            return;
        }
    };
    let family: ZbdFamily = decided.record().family();
    *out.zbd_files_by_family.entry(family.as_str()).or_insert(0) += 1;

    if !indexed_by_trailer(family) {
        return;
    }
    let mut context = ParseContext::with_defaults(spelling);
    let index = match read_version_one_index(&mut context, decided, bytes) {
        Ok(index) => index,
        Err(error) => {
            out.zbd_index_failures
                .push(format!("{spelling}: index refused: {}", error.code()));
            return;
        }
    };
    let table = index.member_table();
    let listing = match list_members(&mut context, bytes, &table) {
        Ok(listing) => listing,
        Err(error) => {
            out.zbd_index_failures
                .push(format!("{spelling}: listing refused: {}", error.code()));
            return;
        }
    };
    out.zbd_indexed_containers += 1;
    for row in listing.rows() {
        let name = String::from_utf8_lossy(row.name()).into_owned();
        out.zbd_indexed_members += 1;
        if let Some((shape, faithful)) = shape_candidate(&name) {
            let span = row.span();
            out.stored_plane_candidates.push(ShapeCandidate {
                location: format!("member {spelling}#{name}"),
                shape,
                template_faithful: faithful,
                offset: Some(span.offset),
                length: Some(span.length),
            });
        }
        if let Some(error) = row.error() {
            out.zbd_member_failures
                .push(format!("{spelling}#{name}: {error:?}"));
        }
    }
}

/// A member path (ROF stores one byte string per path segment) as the
/// `/`-joined spelling the container itself declares.
fn join_member_path(path: &[&[u8]]) -> String {
    path.iter()
        .map(|segment| String::from_utf8_lossy(segment).into_owned())
        .collect::<Vec<_>>()
        .join("/")
}
