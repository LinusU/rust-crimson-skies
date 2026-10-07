//! F64-B retail acceptance tests: the measured legacy-import surface
//! (`specs/F64-legacy-custom-aircraft-and-optional-save-import.md`, stage
//! `### F64-B`, capability `retail`).
//!
//! These tests read the owner's original installation read-only and measure
//! *where the legacy artifacts would live and what references them* — the
//! evidence the inventory rows of F64-A could not carry because that stage
//! ran without `retail`. Every test fails loudly when `CS_GAME_DIR` is unset,
//! and none writes into the installation.
//!
//! What they establish, honestly bounded:
//!
//! * **No legacy file ships.** The inventory lists no `SavedGames\`, no
//!   `Planes\`, no `*.sav`, no `Status.dat`, no `Mission.*`/`Persist.*` file —
//!   the originals are runtime-created, so no byte layout has ever been
//!   measured and every inventory row's `evidence` stays `Unknown`.
//! * **The storage paths are original file content.** The owner-supplied
//!   decrypted engine image holds `Planes\%s`, `SavedGames\%s\...`,
//!   `Status.dat`/`Mission.*`/`Persist.*` templates and the registry key —
//!   where a saved plane or save *would* go, never what it contains.
//! * **Custom aircraft are referenced.** `PLANECONSTRUCTION.SCRIPT` fills a
//!   four-slot grid of saved planes through engine callback 2243 — the
//!   measured reference that turned `CustomAircraft`'s import requirement on.

#[path = "f64_b_support/mod.rs"]
mod f64_b_support;

use cs_formats::legacy_profile::{LegacyArtifactClass, layout_record};
use f64_b_support::{
    ABSENT_SHAPES, ENGINE_IMAGE, PLANE_CONSTRUCTION, PLANE_SLOT_MARKERS, STORAGE_TEMPLATES,
    engine_image, find, game_dir, inventoried_spellings, read_member,
};

/// No legacy save or custom-plane file ships with the installation: every
/// inventoried spelling is checked against the storage shapes the engine
/// image's own templates name. A byte layout can therefore never have been
/// measured — which is exactly what the inventory rows' `Unknown` evidence
/// says.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f64_b_retail_the_installation_ships_no_legacy_save_or_plane_file() {
    let root = game_dir();
    let spellings = inventoried_spellings(&root);
    assert!(
        !spellings.is_empty(),
        "the inventory must list the installation's files"
    );

    for shape in ABSENT_SHAPES {
        let hits: Vec<&String> = spellings
            .iter()
            .filter(|spelling| {
                // The engine spells paths with backslashes; the inventory
                // preserves the host's separators, so both are checked.
                spelling.contains(shape) || spelling.contains(&shape.replace('\\', "/"))
            })
            .collect();
        assert!(
            hits.is_empty(),
            "no inventoried file may match {shape:?} — legacy saves and \
             planes are runtime-created; found {hits:?}"
        );
    }
}

/// The storage paths are measured original file content: every template the
/// import surface needs is found inside the owner-supplied decrypted engine
/// image, so the save/plane *locations* are measured even though no file of
/// those classes ships to read.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f64_b_retail_storage_paths_are_measured_in_the_engine_image() {
    let root = game_dir();
    let image = engine_image(&root);
    assert!(
        image.len() > 1_000_000,
        "{ENGINE_IMAGE} is the full decrypted engine image"
    );

    for (template, meaning) in STORAGE_TEMPLATES {
        assert!(
            find(&image, template.as_bytes()).is_some(),
            "the engine image must hold {template:?} ({meaning})"
        );
    }

    // The `Planes\%s` template appears twice: once beside `rb` (the engine
    // reads listed saved planes) and once beside `wb+` and a bare `Planes\`
    // directory string — the write path that creates them. A file the engine
    // opens for writing is not one it ships, which is why the saved-plane
    // directory is runtime-created.
    let mut planes = Vec::new();
    let mut cursor = 0;
    while let Some(offset) = find(&image[cursor..], b"Planes\\%s") {
        planes.push(cursor + offset);
        cursor += offset + 1;
    }
    assert!(
        planes.len() >= 2,
        "the engine holds both a read and a write path for saved planes"
    );
    let write_path = planes.iter().any(|offset| {
        let window = &image[*offset..(*offset + 64).min(image.len())];
        find(window, b"wb+").is_some() && find(window, b"Planes\x00").is_some()
    });
    assert!(
        write_path,
        "one Planes\\%s must sit beside wb+ and the Planes directory string"
    );
    let read_path = planes
        .iter()
        .any(|offset| find(&image[*offset..(*offset + 64).min(image.len())], b"rb").is_some());
    assert!(read_path, "one Planes\\%s must sit beside the rb read mode");
}

/// `PLANECONSTRUCTION.SCRIPT` references stored custom planes: the four-slot
/// grid, the per-slot plane label and the fill callback are all measured in
/// the shipped member. This is the reference that switched
/// `LEGACY_LAYOUT_INVENTORY[CustomAircraft]`'s requirement on, so the test
/// also asserts the inventory row now records it.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f64_b_retail_custom_aircraft_is_referenced_by_the_construction_screen() {
    let root = game_dir();
    let member = read_member(&root, PLANE_CONSTRUCTION);
    assert!(
        member.len() > 1_000,
        "{PLANE_CONSTRUCTION} is the whole construction screen"
    );

    for (marker, meaning) in PLANE_SLOT_MARKERS {
        assert!(
            find(&member, marker.as_bytes()).is_some(),
            "{PLANE_CONSTRUCTION} must hold {marker:?} ({meaning})"
        );
    }

    let aircraft = layout_record(LegacyArtifactClass::CustomAircraft).requirement;
    assert!(
        aircraft.is_required(),
        "the measured construction-screen reference turns the requirement on"
    );
    assert_eq!(
        aircraft.referenced_by(),
        &[PLANE_CONSTRUCTION],
        "the inventory must record the measured referencing path"
    );
}

/// The save classes stay separately labeled optional enhancements even though
/// their storage locations are now measured: an optional enhancement is never
/// required no matter what references it, and the disable switch is the named
/// contract the F64-A tests pin.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f64_b_retail_save_classes_stay_optional_enhancements() {
    for class in [
        LegacyArtifactClass::CampaignSave,
        LegacyArtifactClass::SettingsBlob,
        LegacyArtifactClass::ProfilePointer,
    ] {
        let requirement = layout_record(class).requirement;
        assert!(
            requirement.is_optional_enhancement() && !requirement.is_required(),
            "{class} stays a separately labeled optional enhancement"
        );
    }
    // And the loadout row keeps its requirement *untriggered*: whether a
    // loadout lives inside a `Planes\` file or separately is unmeasured, so
    // no path can be recorded against it yet.
    let loadout = layout_record(LegacyArtifactClass::CustomLoadout).requirement;
    assert!(
        !loadout.is_required(),
        "no original path has been measured to reference a custom loadout \
         separately from the plane file it may live inside"
    );
}
