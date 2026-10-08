//! F64-D retail acceptance: the recorded custom-aircraft reference still
//! resolves in the owner's installation, and the optional save classes are
//! scoped by the switch over the shipped files (capability `retail`).
//!
//! `specs/F64-legacy-custom-aircraft-and-optional-save-import.md`, stage
//! `### F64-D`. These tests read `$CS_GAME_DIR` read-only — no write path in
//! the import consumer can reach the installation — and fail loudly when
//! `CS_GAME_DIR` is unset.
//!
//! What they establish, honestly bounded:
//!
//! * **The inventory's reference is a live pointer, not a note.** The
//!   `CustomAircraft` row records
//!   `ASSETS/SCRIPTS/PLANECONSTRUCTION.SCRIPT` as the measured original
//!   content path that made the class required; this test resolves that path
//!   in today's manifest and re-reads the shipped member's saved-plane slot
//!   code through the production ROF mount.
//! * **The optional saves are explicitly scoped.** Every file this
//!   installation ships is offered to the production consumer as a save class
//!   with the switch off (refused by name, before any byte is judged), with
//!   the switch on (refused for the unmeasured layout), and the required
//!   custom-aircraft class with the switch off (the switch never applies to
//!   it). Nothing ships that could be mistaken for an old save.

#[path = "f64_c_support/mod.rs"]
mod f64_c;
#[path = "f64_d_support/mod.rs"]
mod f64_d;

use cs_app::ui::import::ImportFlow;
use cs_content::legacy_import::LayoutAdmission;
use cs_formats::legacy_profile::{
    ArtifactProposal, LegacyArtifactClass, MAX_LEGACY_SOURCE_BYTES, layout_record,
};
use cs_types::evidence::ContentHash;
use f64_c::{CatalogRows, TempBase, catalog, id_map, offer, proposal, stock, tree};
use f64_d::{
    ABSENT_SHAPES, BASE_CONTAINER, PLANE_SLOT_MARKERS, context, find, find_spelling, game_dir,
    installation, inventoried_spellings, read_reference,
};

/// The `CustomAircraft` inventory row records an original content path; this
/// resolves it against the installation and re-reads the saved-plane slot code
/// that made the row required.
///
/// The recorded path is an **asset spelling**, not a file at the installation
/// root: it resolves through the production ROF mount of the shipped base
/// container, which is asserted to be an inventoried file. A path nothing in
/// this installation resolves is a stale note, and this test fails on it.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f64_d_retail_the_custom_aircraft_reference_resolves_to_the_shipped_construction_screen() {
    let root = game_dir();
    let manifest = installation(&root);
    let requirement = layout_record(LegacyArtifactClass::CustomAircraft).requirement;
    assert!(
        requirement.is_required(),
        "the measured construction-screen reference keeps this class required"
    );
    let referenced = requirement.referenced_by();
    assert!(
        !referenced.is_empty(),
        "a required class records what references it"
    );
    assert!(
        find_spelling(&manifest, BASE_CONTAINER).is_some(),
        "{BASE_CONTAINER} ships and is inventoried, so its members can be read"
    );

    for recorded in referenced {
        let (source, member) = read_reference(&root, &manifest, recorded).unwrap_or_else(|| {
            panic!(
                "the inventory records {recorded:?} as a referencing original path, but nothing \
                 in {} resolves it: no inventoried file spells it and {} holds no such member",
                manifest.files.len(),
                BASE_CONTAINER
            )
        });
        let label = source.label();
        assert!(
            member.len() > 1_000,
            "{label} is the whole construction screen"
        );
        for (marker, meaning) in PLANE_SLOT_MARKERS {
            assert!(
                find(&member, marker.as_bytes()).is_some(),
                "{label} must hold {marker:?} ({meaning})"
            );
        }
    }
}

/// The optional save classes are scoped by `cs.profile.legacy_save_import`,
/// and no shipped file can reach a report whether the switch is on or off.
///
/// With the switch **off**, every shipped save-class offer is declined with
/// `enhancement_disabled` before the layout capability is even asked (the
/// ordering this stage repaired). With it **on**, the same offers fall
/// through to `no_measured_layout`, which is the honest state of a build that
/// has measured no byte layout. The required custom-aircraft class is offered
/// with the switch off and never sees the switch at all.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f64_d_retail_optional_saves_are_switchable_and_no_shipped_file_reaches_a_report() {
    let root = game_dir();
    let manifest = installation(&root);
    assert!(
        !manifest.files.is_empty(),
        "the installation inventories its files"
    );

    // Not one shipped file is an old save or a stored plane: those are
    // runtime-created, which is exactly why this stage can verify the switch
    // over original content but cannot claim an original byte layout.
    let spellings = inventoried_spellings(&root);
    for shape in ABSENT_SHAPES {
        let native = shape.to_lowercase();
        let forward = shape.replace('\\', "/").to_lowercase();
        let hits: Vec<&String> = spellings
            .iter()
            .filter(|spelling| {
                let lowered = spelling.to_lowercase();
                lowered.contains(&native) || lowered.contains(&forward)
            })
            .collect();
        assert!(
            hits.is_empty(),
            "no shipped file may be an old save or stored plane ({shape:?}): {hits:?}"
        );
    }

    let ids = id_map();
    let content_catalog = catalog(CatalogRows::Base);
    let (rules, policy, book) = stock();
    let target = f64_c::target();
    let off = context(
        &ids,
        &content_catalog,
        &rules,
        &policy,
        &book,
        false,
        LayoutAdmission::MeasuredOnly,
    );
    let on = context(
        &ids,
        &content_catalog,
        &rules,
        &policy,
        &book,
        true,
        LayoutAdmission::MeasuredOnly,
    );

    let base = TempBase::new("f64-d-retail");
    let destination = base.path().join("userdata/production");
    std::fs::create_dir_all(&destination).expect("the destination root is created");
    let probe = destination.join("existing.save");
    std::fs::write(&probe, b"fresh-engine-save").expect("the probe save is written");
    let destination_before = tree(&destination);

    let mut flow = ImportFlow::new();
    let mut offered = 0usize;
    let mut over_cap = 0usize;
    let mut changed: Vec<String> = Vec::new();
    let mut mismatched: Vec<String> = Vec::new();

    for file in manifest.files.iter() {
        if file.size_bytes > MAX_LEGACY_SOURCE_BYTES {
            over_cap += 1;
            let declared = ArtifactProposal::new(
                file.relative_spelling.as_str(),
                file.size_bytes,
                ContentHash::from_bytes([0u8; 32]),
                Some(LegacyArtifactClass::CampaignSave),
            );
            assert!(
                declared.is_err(),
                "{} is over the import cap and must be refused at the proposal",
                file.relative_spelling
            );
            continue;
        }
        let bytes = std::fs::read(root.join(file.relative_spelling.as_str()))
            .unwrap_or_else(|error| panic!("{} must be readable: {error}", file.relative_spelling));
        let source = proposal(&bytes, LegacyArtifactClass::CampaignSave).unwrap_or_else(|error| {
            panic!("{} must be proposable: {error}", file.relative_spelling)
        });

        let disabled = flow
            .offer(&off, &offer(&source, &bytes, None, None, &target))
            .code();
        if disabled != "enhancement_disabled" {
            mismatched.push(format!(
                "{}: switch off gave {disabled}, expected enhancement_disabled",
                file.relative_spelling
            ));
        }

        let unmeasured = flow
            .offer(&on, &offer(&source, &bytes, None, None, &target))
            .code();
        if unmeasured != "no_measured_layout" {
            mismatched.push(format!(
                "{}: switch on gave {unmeasured}, expected no_measured_layout",
                file.relative_spelling
            ));
        }

        let after = std::fs::read(root.join(file.relative_spelling.as_str())).expect("re-read");
        if after != bytes {
            changed.push(file.relative_spelling.as_str().to_owned());
        }
        offered += 1;
    }
    assert!(offered > 0, "at least one shipped file is within the cap");
    assert_eq!(
        offered + over_cap,
        manifest.files.len(),
        "every inventoried file is either offered within the cap or refused above it"
    );
    assert!(
        mismatched.is_empty(),
        "every offer must be declined by the switch and by the unmeasured layout: {mismatched:?}"
    );
    assert!(changed.is_empty(), "sources changed: {changed:?}");

    // The required class with the switch off: the switch is not in its path,
    // so the missing layout — never `enhancement_disabled` — is the answer.
    let plane_source = manifest
        .files
        .iter()
        .find(|file| file.size_bytes <= MAX_LEGACY_SOURCE_BYTES)
        .expect("at least one file is within the cap");
    let bytes = std::fs::read(root.join(plane_source.relative_spelling.as_str())).expect("read");
    let source =
        proposal(&bytes, LegacyArtifactClass::CustomAircraft).expect("the file is proposable");
    let code = flow
        .offer(&off, &offer(&source, &bytes, None, None, &target))
        .code();
    assert_eq!(
        code, "no_measured_layout",
        "the optional-save switch never applies to the required class"
    );

    let destination_after = tree(&destination);
    let writes = destination_after
        .iter()
        .filter(|row| !destination_before.contains(row))
        .count();
    assert_eq!(writes, 0, "the consumer wrote into the destination");
    assert_eq!(
        std::fs::read(&probe).expect("the probe file is still there"),
        b"fresh-engine-save".to_vec(),
        "the consumer rewrote the probe save"
    );
}
