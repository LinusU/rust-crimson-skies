//! T582 acceptance tests: the measured legacy custom-aircraft surface of the
//! retail installation (Rally task #582, "Measure the legacy custom-aircraft
//! layout before a custom-plane catalog collection").
//!
//! F64-B measured that no inventoried **file spelling** matches the storage
//! shapes the engine image's own templates name, and that
//! `PLANECONSTRUCTION.SCRIPT` references stored planes. What neither that
//! sweep nor F14-D.8's catalog stage established is whether a legacy
//! custom-aircraft artifact ships **inside a container**. These tests close
//! that gap:
//!
//! * the retail test (`#[ignore = "requires CS_GAME_DIR"]`) runs the
//!   installation-wide census of `t582_support` — every `.rof` member
//!   decoded and every `.zbd` file dispatched and listed through production
//!   readers — and asserts that every file is classified, every member is
//!   accounted for, and not one spelling or member name matches a
//!   stored-plane/save storage shape. That is the measured negative: the
//!   installation ships **no** legacy custom aircraft;
//! * the fabrication guard pins, over production `retail_baseline`, that a
//!   file *named* like a stored plane never becomes a `ContentKind::CustomPlane`
//!   row or collection record — the negative can never be "resolved" by
//!   inferring a format from a name (AGENTS rule 4);
//! * the inventory guard pins that the measured referencing path stays
//!   recorded and the layout evidence stays `Unknown`: the byte layout of a
//!   stored plane is measured by nothing, so nothing may claim it.
//!
//! Without `CS_GAME_DIR` the retail test fails loudly rather than passing
//! vacuously. No test writes into the installation.

#[path = "t582_support/mod.rs"]
mod t582_support;

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use cs_content::catalog::baseline::{Baseline, install_file_key, retail_baseline};
use cs_formats::legacy_profile::{LegacyArtifactClass, layout_record};
use cs_types::content::{ContentId, ContentKind};
use cs_types::evidence::ClaimStatus;
use t582_support::{Census, census, game_dir};

/// The member F64-B measured to reference stored custom planes.
const PLANE_CONSTRUCTION: &str = "ASSETS/SCRIPTS/PLANECONSTRUCTION.SCRIPT";

// ------------------------------------------------------------ temp tree ---

/// A disposable installation tree, removed on drop (the F14-D.8 fixture
/// pattern, trimmed to what the campaign walk needs).
struct TempInstall(PathBuf);

impl TempInstall {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-t582-{label}-{}-{}",
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

/// A `.zrd` integer node: tag `1` and the value (the measured grammar the
/// F14-D.8 fixtures encode).
fn zrd_int(value: u32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&value.to_le_bytes());
    bytes
}

/// A version-one reader archive: the member data, then one 148-byte index
/// entry each, then the u32 version `1` and u32 count (the measured shape
/// the F14-D.8 fixtures encode).
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

/// The minimal authored campaign tree the shared campaign walk declares `c1`
/// from: one mission's reader archive, its mission animation and the group's
/// shared reader.
fn campaign_tree(label: &str) -> TempInstall {
    let temp = TempInstall::new(label);
    temp.write(
        "ZBD/C1/M01/zrdr.zbd",
        &reader_archive(&[
            ("net.zrd", zrd_int(1)),
            ("map.zrd", zrd_int(1)),
            ("aiv.zrd", zrd_int(1)),
            ("objectives.zrd", zrd_int(1)),
        ]),
    );
    temp.write("ZBD/C1/M01/mis_anim.zbd", b"mission animation bytes");
    temp.write(
        "ZBD/C1/zrdr.zbd",
        &reader_archive(&[("templates.zrd", zrd_int(1)), ("cam_anim.zrd", zrd_int(1))]),
    );
    temp
}

// --------------------------------------------------------------- helpers ---

/// Every row of one kind, in catalog order.
fn rows_of(baseline: &Baseline, kind: ContentKind) -> Vec<&cs_types::content::CatalogElement> {
    baseline
        .catalog
        .elements()
        .filter(|element| element.kind == kind)
        .collect()
}

/// The collection record of one kind, when the baseline wrote one.
fn status_of(
    baseline: &Baseline,
    kind: ContentKind,
) -> Option<&cs_content::catalog::baseline::CollectionStatus> {
    baseline
        .collection_status
        .iter()
        .find(|status| status.kind == kind)
}

/// Every id the catalog holds, for containment checks.
fn ids(baseline: &Baseline) -> BTreeSet<String> {
    baseline
        .catalog
        .elements()
        .map(|element| element.id.as_str().to_owned())
        .collect()
}

// ------------------------------------------------------ fabrication guard ---

/// A file **named** like a stored plane, or hidden in a `Planes`/`SavedGames`
/// directory, never becomes a `custom_plane` row or collection record. The
/// whole measured-negative verdict rests on this: the layout of a stored
/// plane is measured by nothing, so the only way to "find" one would be to
/// infer a format from a name — which the catalog refuses. The stray files
/// really are inventoried (their `install_file` rows are asserted present),
/// so the empty `custom_plane` result cannot pass by observing nothing.
#[test]
fn accept_t582_a_file_named_like_a_stored_plane_never_becomes_a_custom_plane_row() {
    let temp = campaign_tree("fabrication");
    temp.write(
        "Planes/TESTPLANE.DAT",
        b"synthetic bytes that resemble nothing measured",
    );
    temp.write(
        "GOSDATA/SAVEDGAMES/SLOT1.SAV",
        b"synthetic bytes that resemble nothing measured",
    );

    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    // The stray files are in the inventory, addressed by their own keys.
    let held = ids(&baseline);
    for spelling in ["Planes/TESTPLANE.DAT", "GOSDATA/SAVEDGAMES/SLOT1.SAV"] {
        let id = ContentId::from_source(ContentKind::InstallFile, &install_file_key(spelling))
            .expect("a valid install-file id");
        assert!(
            held.contains(id.as_str()),
            "{spelling} must be inventoried as its own install_file row, so the \
             empty custom_plane result below is not vacuous"
        );
    }

    assert!(
        rows_of(&baseline, ContentKind::CustomPlane).is_empty(),
        "no custom_plane row may be fabricated from a file name or directory"
    );
    assert!(
        status_of(&baseline, ContentKind::CustomPlane).is_none(),
        "with no located source there is no custom_plane collection record \
         either (the F14-D.6/F14-D.8 no-source-no-record precedent)"
    );
    let report = cs_content::catalog::baseline::baseline_report_json(&baseline);
    assert!(
        !report.contains("\"custom_plane\""),
        "the report must carry no custom_plane collection key"
    );
}

// ------------------------------------------------------- inventory guard ---

/// The measured state of the custom-aircraft inventory row stays exactly as
/// F64-B measured it: the construction screen's referencing path is
/// recorded, the requirement is on, and the **layout** evidence stays
/// `Unknown` — this task measured no stored-plane bytes, because none ship,
/// so nothing may upgrade the row. `CustomPlane` also stays non-launchable,
/// so populating the catalog's denominator is impossible from here.
#[test]
fn accept_t582_the_inventory_row_keeps_its_measured_reference_and_unknown_layout() {
    let row = layout_record(LegacyArtifactClass::CustomAircraft);
    assert!(
        row.requirement.is_required(),
        "the measured construction-screen reference keeps the requirement on"
    );
    assert_eq!(
        row.requirement.referenced_by(),
        &[PLANE_CONSTRUCTION],
        "the inventory records the one measured referencing member"
    );
    assert_eq!(
        row.evidence,
        ClaimStatus::Unknown,
        "no stored-plane file ships, so no byte layout has been measured and \
         the row's layout evidence must stay Unknown"
    );
    assert!(
        !row.unknowns.is_empty(),
        "the open questions stay listed beside the unknown layout"
    );
    assert!(
        !ContentKind::CustomPlane.is_launchable(),
        "a custom plane is never launchable content: the coverage \
         denominator cannot move by populating the collection"
    );
}

// ------------------------------------------------------- retail census ---

/// Every inventoried file is classified by production readers, every
/// container member is accounted for, and not one spelling or member name
/// matches the engine's own stored-plane/save storage shapes: the
/// installation ships **no** legacy custom aircraft. The counts are the
/// owner installation's corpus pins, so a corpus change fails loudly here
/// instead of silently moving the verdict.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t582_retail_every_container_member_is_accounted_for_and_no_stored_plane_ships() {
    let root = game_dir();
    let census = census(&root);
    print_summary(&census);

    assert!(
        census.file_count >= 200,
        "production discovery must inventory the installation"
    );
    assert!(
        census.unclassified.is_empty(),
        "every inventoried file must be classified by a production reader: {:?}",
        census.unclassified
    );

    // The two ROF containers, fully decoded member by member.
    assert_eq!(
        census.rof_containers.len(),
        2,
        "the installation ships exactly two ROF containers"
    );
    let rof_members: usize = census.rof_containers.iter().map(|rof| rof.members).sum();
    assert_eq!(
        rof_members, 847,
        "the two ROF containers declare 847 members together"
    );
    for rof in &census.rof_containers {
        assert_eq!(
            rof.members, rof.decoded_ok,
            "every member of {} must decode through the production member \
             reader: {:?}",
            rof.spelling, rof.decode_failures
        );
        assert!(
            rof.decode_failures.is_empty(),
            "{} must have no member decode failures: {:?}",
            rof.spelling,
            rof.decode_failures
        );
    }

    // Every ZBD file dispatched to a family; the trailer-indexed families
    // list every member readable.
    let family_total: usize = census.zbd_files_by_family.values().sum();
    assert_eq!(
        family_total,
        census.file_count - census.plain_files.len() - census.rof_containers.len(),
        "every container file is either ROF or a dispatched ZBD"
    );
    assert!(
        census.zbd_index_failures.is_empty(),
        "every trailer-indexed container must list: {:?}",
        census.zbd_index_failures
    );
    assert_eq!(
        census.zbd_indexed_members, 6334,
        "the sound and reader containers declare 6334 members"
    );
    assert!(
        census.zbd_member_failures.is_empty(),
        "every indexed member must be readable: {:?}",
        census.zbd_member_failures
    );

    // The verdict: nothing the engine's own templates could open ships as a
    // file or a container member. Substring hits are still recorded as
    // candidates with their byte evidence — the six measured on the owner
    // installation are all `MISSION.` inside a graphics or lobby-script
    // member name, none of them a save — and are listed by name in
    // `docs/findings/2026-10-12-t582-legacy-custom-aircraft-census.md`.
    let faithful: Vec<&t582_support::ShapeCandidate> = census
        .stored_plane_candidates
        .iter()
        .filter(|candidate| candidate.template_faithful)
        .collect();
    assert!(
        faithful.is_empty(),
        "no file spelling or container member may be a spelling the engine's \
         own storage templates could open: {faithful:?}"
    );
    let mut candidates: Vec<String> = census
        .stored_plane_candidates
        .iter()
        .map(|candidate| candidate.location.clone())
        .collect();
    candidates.sort();
    assert_eq!(
        candidates,
        [
            "member GOSDATA/ASSETS/crimson.rof#ASSETS/GRAPHICS/FC_B_FLYMISSION.PNG",
            "member GOSDATA/ASSETS/crimson.rof#ASSETS/GRAPHICS/IA_B_FLYNEWMISSION.PNG",
            "member GOSDATA/ASSETS/crimson.rof#ASSETS/GRAPHICS/IA_B_WEAPONLOADOUTFLYMISSION.PNG",
            "member GOSDATA/ASSETS/crimson.rof#ASSETS/GRAPHICS/MP_LOBBY_MISSION.PNG",
            "member GOSDATA/ASSETS/crimson.rof#ASSETS/GRAPHICS/PC_B_NEXTMISSION.PNG",
            "member GOSDATA/ASSETS/crimson.rof#ASSETS/SCRIPTS/MULTIPLAYERLOBBY_MISSION.SCRIPT",
        ],
        "the substring candidates are exactly the six measured names, each \
         recorded with its stored extent"
    );
    assert!(
        census
            .stored_plane_candidates
            .iter()
            .all(|candidate| candidate.offset.is_some() && candidate.length.is_some()),
        "every candidate carries its stored extent as byte evidence"
    );

    // The measured reference is inside the container the census decoded: the
    // construction-screen member really ships, and the inventory row still
    // records it.
    let base = census
        .rof_containers
        .iter()
        .find(|rof| rof.spelling == t582_support::BASE_CONTAINER)
        .expect("the shared archive is inventoried");
    assert!(
        base.member_spellings
            .iter()
            .any(|member| member == PLANE_CONSTRUCTION),
        "{PLANE_CONSTRUCTION} must ship inside {}",
        base.spelling
    );
}

/// The measured counts, printed for the findings note and the evidence log.
fn print_summary(census: &Census) {
    println!("install_sha256 {}", census.install_sha256);
    println!("content_sha256 {}", census.content_sha256);
    println!("files {}", census.file_count);
    for rof in &census.rof_containers {
        println!(
            "rof {} members {} decoded_ok {}",
            rof.spelling, rof.members, rof.decoded_ok
        );
    }
    for (family, files) in &census.zbd_files_by_family {
        println!("zbd family {family} files {files}");
    }
    println!(
        "zbd indexed containers {} members {}",
        census.zbd_indexed_containers, census.zbd_indexed_members
    );
    println!("plain files {}", census.plain_files.len());
    println!("unclassified {}", census.unclassified.len());
    for candidate in &census.stored_plane_candidates {
        println!(
            "candidate {} shape {:?} template_faithful {} offset {:?} length {:?}",
            candidate.location,
            candidate.shape,
            candidate.template_faithful,
            candidate.offset,
            candidate.length
        );
    }
}
