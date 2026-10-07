//! `accept_f17_d_` tests for the original-data comparison matrix and its
//! material coverage: `cs_app::render::matrix`
//! (`specs/F17-rendering-material-fidelity-and-scalable-presentation.md`,
//! section `### F17-D`, AC04).
//!
//! The fast tests pin the *contract*: the set is exactly AC04's five
//! subjects, a set that is missing one or carrying one twice is refused, a
//! subject that could not be resolved stays in the set carrying its refusal,
//! every selection rule refuses by name when the forest holds nothing it
//! asks for, and material coverage accounts for every stored record without
//! ever inventing a class.
//!
//! The `#[ignore]`d tests are the stage's `retail` and `gpu` halves: they
//! read the owner's installation through the production readers, resolve all
//! five subjects against it, and draw each one on a real adapter. They are
//! ignored in CI (no original data there) and **fail loudly** without
//! `CS_GAME_DIR`.

use std::path::{Path, PathBuf};

use cs_app::render::material::{DeclaredClass, MaterialClass, MaterialFacts};
use cs_app::render::matrix::{
    COCKPIT_ANCHOR, ComparisonMatrix, ComparisonSubject, MATRIX_WORLD_GROUP, MaterialCoverage,
    MatrixContainer, MatrixError, MatrixRow, ResolvedSubject, SubjectNode, resolve_all, select,
};
use cs_formats::gamez::{MATERIAL_FLAG_TEXTURED, RawMaterialRecord};
use cs_types::evidence::ClaimStatus;

// ------------------------------------------------------------- synthetic ---

fn node(slot: u32, name: &str, parent: Option<u32>, children: Vec<u32>, mesh: i32) -> SubjectNode {
    SubjectNode {
        slot,
        name: name.to_owned(),
        parent,
        children,
        mesh_index: mesh,
        is_world_record: false,
    }
}

fn world(slot: u32, name: &str, children: Vec<u32>) -> SubjectNode {
    SubjectNode {
        slot,
        name: name.to_owned(),
        parent: None,
        children,
        mesh_index: -1,
        is_world_record: true,
    }
}

fn unresolved(subject: ComparisonSubject) -> MatrixRow {
    MatrixRow::unresolved(
        subject,
        MatrixError::InstancedChildrenAbsent {
            container: "fixture".to_owned(),
        },
    )
}

/// AC04 names five subjects: `cockpit`, `skyline`, `vegetation`,
/// `night_effects`, `close_up_aircraft`.
///
/// The set is the acceptance scenario itself, so its size, its order and its
/// identifiers are pinned: dropping a subject from the enum, renaming one or
/// reordering `ALL` fails here rather than quietly shrinking the comparison.
#[test]
fn accept_f17_d_the_comparison_set_is_exactly_the_five_required_subjects() {
    let codes: Vec<&str> = ComparisonSubject::ALL
        .iter()
        .map(|subject| subject.code())
        .collect();
    assert_eq!(
        codes,
        [
            "cockpit",
            "skyline",
            "vegetation",
            "night_effects",
            "close_up_aircraft"
        ],
        "AC04's five subjects, in the sheet's order"
    );
    assert_eq!(ComparisonSubject::ALL.len(), 5);

    let mut indices: Vec<usize> = ComparisonSubject::ALL
        .iter()
        .map(|subject| subject.index())
        .collect();
    indices.sort_unstable();
    assert_eq!(
        indices,
        [0, 1, 2, 3, 4],
        "every subject owns its own coverage bucket"
    );

    let sides: Vec<(&str, bool)> = ComparisonSubject::ALL
        .iter()
        .map(|subject| {
            (
                subject.code(),
                matches!(subject.side(), cs_app::render::matrix::SubjectSide::World),
            )
        })
        .collect();
    assert_eq!(
        sides,
        [
            ("cockpit", false),
            ("skyline", true),
            ("vegetation", true),
            ("night_effects", true),
            ("close_up_aircraft", false),
        ],
        "the three world-side subjects read the world container, the two airframe subjects the \
         shared airframe container"
    );
}

/// A set without one required subject, and a set with one subject twice, are
/// both refused — the failure cases behind "the set includes all five".
#[test]
fn accept_f17_d_a_set_missing_or_duplicating_a_required_subject_is_refused() {
    let four: Vec<MatrixRow> = ComparisonSubject::ALL
        .iter()
        .copied()
        .filter(|subject| *subject != ComparisonSubject::NightEffects)
        .map(unresolved)
        .collect();
    assert_eq!(four.len(), 4);
    let error = ComparisonMatrix::build(four).expect_err("four rows are not the comparison set");
    assert_eq!(
        error,
        MatrixError::MissingSubject {
            subject: ComparisonSubject::NightEffects,
        }
    );
    assert!(
        error.to_string().contains("night_effects"),
        "the refusal names the subject that is missing: {error}"
    );

    let mut six: Vec<MatrixRow> = ComparisonSubject::ALL
        .iter()
        .copied()
        .map(unresolved)
        .collect();
    six.push(unresolved(ComparisonSubject::Skyline));
    let error = ComparisonMatrix::build(six).expect_err("six rows carry a subject twice");
    assert_eq!(
        error,
        MatrixError::DuplicateSubject {
            subject: ComparisonSubject::Skyline,
        }
    );

    let five = ComparisonMatrix::build(
        ComparisonSubject::ALL
            .iter()
            .copied()
            .map(unresolved)
            .collect(),
    )
    .expect("five distinct subjects are the set");
    assert_eq!(five.rows().len(), 5);
}

/// An unresolved subject is a row, not a hole: the set still includes it, its
/// refusal is readable, and the matrix says how many did resolve instead of
/// pretending everything did.
#[test]
fn accept_f17_d_an_unresolved_subject_stays_in_the_set_with_its_reason() {
    let refusal = MatrixError::AnchorAbsent {
        subject: ComparisonSubject::NightEffects,
        container: "zbd/c1c/gamez.zbd".to_owned(),
        expected: "moon|stars".to_owned(),
        within: "the whole world node array".to_owned(),
    };
    let mut rows: Vec<MatrixRow> = ComparisonSubject::ALL
        .iter()
        .copied()
        .map(unresolved)
        .collect();
    rows[3] = MatrixRow::unresolved(ComparisonSubject::NightEffects, refusal);

    let matrix = ComparisonMatrix::build(rows).expect("all five subjects are present");
    assert_eq!(matrix.resolved_count(), 0);
    assert!(!matrix.is_fully_resolved());
    let row = matrix
        .row(ComparisonSubject::NightEffects)
        .expect("the refused subject is still a row of the set");
    let reason = row.reason().expect("the row carries its refusal");
    assert!(
        reason.contains("moon|stars") && reason.contains("zbd/c1c/gamez.zbd"),
        "the refusal says what was looked for and where: {reason}"
    );
    assert!(
        matrix
            .rows()
            .iter()
            .any(|row| row.subject() == ComparisonSubject::NightEffects),
        "a refused subject must never be dropped from the set"
    );
}

/// Every selection rule refuses by name when the stored forest holds nothing
/// it asks for: no world record, an absent anchor, an anchor with no mesh, an
/// uninstanced or ambiguous vegetation family.
///
/// These are the failure cases of the retail resolution: the same code paths
/// that would run against the installation if a stored name moved.
#[test]
fn accept_f17_d_a_selection_rule_that_finds_nothing_is_refused_by_name() {
    // No world record at all.
    let orphan = vec![node(0, "dome", None, vec![], 3)];
    let error = select(ComparisonSubject::Skyline, "fixture", &orphan)
        .expect_err("a container with no world record cannot anchor a world subject");
    assert!(matches!(error, MatrixError::NoWorldRecord { found: 0, .. }));

    // Two world records: the rule allows exactly one.
    let two = vec![world(0, "world1", vec![2]), world(1, "world2", vec![2])];
    let error = select(ComparisonSubject::Skyline, "fixture", &two)
        .expect_err("two world records are ambiguous");
    assert!(matches!(error, MatrixError::NoWorldRecord { found: 2, .. }));

    // The world record exists but has no `horizon` child.
    let no_horizon = vec![
        world(0, "world1", vec![1]),
        node(1, "hill", Some(0), vec![], -1),
    ];
    let error = select(ComparisonSubject::Skyline, "fixture", &no_horizon)
        .expect_err("the skyline anchor is absent");
    assert_eq!(
        error,
        MatrixError::AnchorAbsent {
            subject: ComparisonSubject::Skyline,
            container: "fixture".to_owned(),
            expected: "horizon".to_owned(),
            within: "the world record's stored child list".to_owned(),
        }
    );
    assert!(
        error.to_string().contains("horizon"),
        "the refusal names the anchor it looked for: {error}"
    );

    // The anchor exists but binds no mesh anywhere below it.
    let no_mesh = vec![
        world(0, "world1", vec![1]),
        node(1, "horizon", Some(0), vec![2], -1),
        node(2, "dome", Some(1), vec![], -1),
    ];
    let error = select(ComparisonSubject::Skyline, "fixture", &no_mesh)
        .expect_err("an anchor with no mesh-bound node cannot be drawn");
    assert_eq!(
        error,
        MatrixError::NoMeshBoundNode {
            subject: ComparisonSubject::Skyline,
            container: "fixture".to_owned(),
            anchor: "horizon".to_owned(),
        }
    );

    // No parentless airframe root carries a `cockpit1` child.
    let no_cockpit = vec![
        node(0, "bloodhawk", None, vec![1], -1),
        node(1, "healthy", Some(0), vec![], 4),
    ];
    let error = select(ComparisonSubject::Cockpit, "fixture", &no_cockpit)
        .expect_err("the cockpit anchor is absent");
    assert_eq!(
        error,
        MatrixError::AnchorAbsent {
            subject: ComparisonSubject::Cockpit,
            container: "fixture".to_owned(),
            expected: COCKPIT_ANCHOR.to_owned(),
            within: "the parentless airframe roots' stored child lists".to_owned(),
        }
    );

    // The world stores no night-sky node at all.
    let no_night = vec![
        world(0, "world1", vec![1]),
        node(1, "hill", Some(0), vec![], 7),
    ];
    let error = select(ComparisonSubject::NightEffects, "fixture", &no_night)
        .expect_err("the night anchors are absent");
    assert!(matches!(
        error,
        MatrixError::AnchorAbsent {
            subject: ComparisonSubject::NightEffects,
            ..
        }
    ));

    // The world record's children are all uniquely named: no instanced family.
    let unique = vec![
        world(0, "world1", vec![1, 2]),
        node(1, "piratezep", Some(0), vec![], -1),
        node(2, "horizon", Some(0), vec![], -1),
    ];
    let error = select(ComparisonSubject::Vegetation, "fixture", &unique)
        .expect_err("no repeated child name means no vegetation family");
    assert_eq!(
        error,
        MatrixError::InstancedChildrenAbsent {
            container: "fixture".to_owned(),
        }
    );

    // Two families repeat: "the instanced family" no longer names one.
    let ambiguous = vec![
        world(0, "world1", vec![1, 2, 3, 4]),
        node(1, "trees", Some(0), vec![], 5),
        node(2, "trees", Some(0), vec![], 6),
        node(3, "rocks", Some(0), vec![], 7),
        node(4, "rocks", Some(0), vec![], 8),
    ];
    let error = select(ComparisonSubject::Vegetation, "fixture", &ambiguous)
        .expect_err("two repeated names do not name one family");
    assert_eq!(
        error,
        MatrixError::AmbiguousInstancedChildren {
            container: "fixture".to_owned(),
            names: vec!["trees".to_owned(), "rocks".to_owned()],
        }
    );
}

/// The rules also *succeed* on a forest that holds what they ask for, and the
/// selection they return names the anchor and every mesh-bound candidate in
/// stored order — which is what the retail run then loads.
#[test]
fn accept_f17_d_a_selection_rule_finds_the_anchor_and_its_candidates() {
    let forest = vec![
        world(0, "world1", vec![1, 4, 5]),
        node(1, "horizon", Some(0), vec![2, 3], -1),
        node(2, "dome", Some(1), vec![], 11),
        node(3, "scroll", Some(1), vec![], -1),
        node(4, "trees", Some(0), vec![6], -1),
        node(5, "trees", Some(0), vec![], 12),
        node(6, "leaf", Some(4), vec![], 13),
    ];

    let skyline = select(ComparisonSubject::Skyline, "fixture", &forest)
        .expect("the world record's `horizon` child anchors the skyline");
    assert_eq!(skyline.anchor_slot, 1);
    assert_eq!(skyline.anchor_name, "horizon");
    let candidates: Vec<&str> = skyline
        .candidates
        .iter()
        .map(|candidate| candidate.name.as_str())
        .collect();
    assert_eq!(
        candidates,
        ["dome"],
        "only mesh-bound descendants are candidates, in stored order"
    );
    assert_eq!(skyline.candidates[0].mesh_index, 11);

    let vegetation = select(ComparisonSubject::Vegetation, "fixture", &forest)
        .expect("the one repeated child name anchors the vegetation family");
    assert_eq!(vegetation.anchor_name, "trees");
    assert_eq!(
        vegetation.anchor_slot, 4,
        "the family's first instance in stored order, not the last"
    );
    let candidates: Vec<&str> = vegetation
        .candidates
        .iter()
        .map(|candidate| candidate.name.as_str())
        .collect();
    assert_eq!(
        candidates,
        ["leaf"],
        "the first instance's own subtree supplies the candidates"
    );
}

/// The coverage table accounts for every stored material record exactly once,
/// groups the refusals by their stable code, and never turns an unclassified
/// material into an opaque one (spec F17 non-negotiable 1).
#[test]
fn accept_f17_d_material_coverage_counts_every_material_and_never_invents_a_class() {
    let raw = RawMaterialRecord {
        alpha: 0xFF,
        flags: MATERIAL_FLAG_TEXTURED,
        rgb: 0x7FFF,
        color: [255.0, 255.0, 255.0],
        texture_index: 5,
        field20: 0.0,
        field24: 0.5,
        field28: 0.5,
        field32: 0.0,
        cycle_ptr: 0,
    };
    let declared = MaterialFacts::declared(
        DeclaredClass::new(MaterialClass::Opaque, ClaimStatus::Designed)
            .expect("Designed asserts a class"),
    );
    let coverage = MaterialCoverage::of([declared, MaterialFacts::for_raw_record(&raw)]);

    assert_eq!(coverage.total(), 2, "both records are counted");
    assert_eq!(coverage.classified(), 1);
    assert_eq!(coverage.count(MaterialClass::Opaque), 1);
    assert_eq!(coverage.unclassified(), 1);
    assert!(coverage.is_complete());
    assert_eq!(
        coverage.reasons().get("undeclared"),
        Some(&1),
        "the stored record's refusal is reported by code: {:?}",
        coverage.reasons()
    );
    assert!(!coverage.is_fully_unclassified(), "one record did classify");

    // The measured shape of original content: a stored record alone asserts
    // no class, so every material is unclassified with its own reason and
    // none of them is counted as any class.
    let original = MaterialCoverage::of([
        MaterialFacts::for_raw_record(&raw),
        MaterialFacts::for_raw_record(&raw),
        MaterialFacts::for_raw_record(&raw),
    ]);
    assert_eq!(original.total(), 3);
    assert_eq!(original.classified(), 0);
    assert_eq!(original.unclassified(), 3);
    assert_eq!(original.count(MaterialClass::Opaque), 0);
    assert!(original.is_complete());
    assert!(original.is_fully_unclassified());
    assert_eq!(original.reasons().get("undeclared"), Some(&3));

    // A refusal that could not happen without the implementation being
    // removed: an empty coverage has counted nothing at all.
    let empty = MaterialCoverage::of([]);
    assert_eq!(empty.total(), 0);
    assert!(empty.is_complete());
    assert!(
        !empty.is_fully_unclassified(),
        "nothing was measured at all"
    );
}

// ---------------------------------------------------------------- retail ---

/// The workspace root, because cargo runs a test binary from the *package*
/// root.
fn workspace_path(relative: &str) -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .join(relative)
}

/// Reads both original containers through the production readers and hands
/// the comparison matrix to `f`. Ignored without `CS_GAME_DIR`, which is set
/// for the retail half of the suite.
fn with_matrix<R>(f: impl FnOnce(&ComparisonMatrix) -> R) -> R {
    let game_dir = PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set for the retail half"),
    );
    let sources = cs_app::playtest_retail::read_playtest_sources(&game_dir, MATRIX_WORLD_GROUP)
        .expect("the world and airframe containers must read");
    let world = MatrixContainer::new(
        sources.world().container_key(),
        sources.world().nodes(),
        sources.world().meshes(),
        sources.world().materials(),
    );
    let aircraft = MatrixContainer::new(
        sources.aircraft().container_key(),
        sources.aircraft().nodes(),
        sources.aircraft().meshes(),
        sources.aircraft().materials(),
    );
    f(&resolve_all(&world, &aircraft))
}

/// AC04's scenario, measured: all five subjects resolve to named original
/// content — a cockpit subtree, the horizon, an instanced vegetation family,
/// the night sky and an intact airframe — and every one of them is a mesh the
/// production adapter can build.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f17_d_retail_every_subject_resolves_to_named_original_content() {
    with_matrix(|matrix| {
        assert_eq!(
            matrix.rows().len(),
            ComparisonSubject::ALL.len(),
            "the set is built from ComparisonSubject::ALL"
        );
        assert!(
            matrix.is_fully_resolved(),
            "every subject must resolve against the installation; refusals: {:?}",
            matrix
                .rows()
                .iter()
                .filter_map(|row| row.reason().map(str::to_owned))
                .collect::<Vec<_>>()
        );

        for subject in ComparisonSubject::ALL {
            let resolved = resolved(matrix, subject);
            assert!(
                resolved.triangles > 0,
                "{subject} drew nothing: {resolved:?}"
            );
            assert!(
                resolved.coverage.is_complete(),
                "{subject}'s coverage does not add up: {:?}",
                resolved.coverage
            );
            assert!(
                resolved.container.contains("gamez.zbd") || resolved.container.contains("planes"),
                "{subject} came from an unexpected container: {}",
                resolved.container
            );
        }

        let cockpit = resolved(matrix, ComparisonSubject::Cockpit);
        assert_eq!(
            cockpit.anchor_name, COCKPIT_ANCHOR,
            "the cockpit subject hangs on the airframe's own `cockpit1` subtree"
        );

        let skyline = resolved(matrix, ComparisonSubject::Skyline);
        assert_eq!(skyline.anchor_name, "horizon");
        assert!(
            skyline.extent.iter().cloned().fold(0.0_f32, f32::max) > 10_000.0,
            "the skyline subject is the measured 17 km horizon dome, not a nearby object: \
             {:?}",
            skyline.extent
        );

        let vegetation = resolved(matrix, ComparisonSubject::Vegetation);
        assert!(
            vegetation.chosen.name.starts_with('g'),
            "the vegetation family's descendants are the instanced `g…` records measured by the \
             world import: {}",
            vegetation.chosen.name
        );

        let night = resolved(matrix, ComparisonSubject::NightEffects);
        assert_eq!(
            night.chosen.name, "moon",
            "of the two night-sky objects, the one that stores geometry is the moon"
        );
        assert!(
            night.skipped.iter().any(|skip| skip.name == "stars"),
            "the star field stores no position and must be reported as passed over, not drawn \
             blank: {:?}",
            night.skipped
        );

        let aircraft = resolved(matrix, ComparisonSubject::CloseUpAircraft);
        assert_eq!(
            aircraft.anchor_name, "healthy",
            "the close-up subject is the airframe's intact state"
        );
        assert!(
            aircraft.container.contains("planes"),
            "{}",
            aircraft.container
        );

        eprintln!("F17-D retail matrix:");
        for row in matrix.rows() {
            if let Some(resolved) = row.resolved() {
                eprintln!(
                    "  {}: {} {} slot {} mesh {} ({} triangles, {} stored units, {} materials, \
                     {} skipped)",
                    resolved.subject,
                    resolved.container,
                    resolved.anchor_name,
                    resolved.chosen.slot,
                    resolved.chosen.mesh_index,
                    resolved.triangles,
                    resolved.extent.iter().cloned().fold(0.0_f32, f32::max),
                    resolved.coverage.total(),
                    resolved.skipped.len(),
                );
            }
        }
    });
}

/// The material-coverage half of the stage, measured: every stored material
/// record of every subject is counted, none of them establishes a render
/// class on its own, and each refusal is the classifier's `undeclared`.
///
/// That is this stage's finding, not a shortfall of the test: no evidence
/// source states a render class for original content yet, so the coverage
/// table says so instead of defaulting the surfaces to opaque.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f17_d_retail_the_comparison_s_material_coverage_is_reported_not_assumed() {
    with_matrix(|matrix| {
        assert!(
            matrix.is_fully_resolved(),
            "the coverage is only reported for a set that resolved"
        );
        for subject in ComparisonSubject::ALL {
            let resolved = resolved(matrix, subject);
            let coverage = &resolved.coverage;
            assert!(
                coverage.total() >= 1,
                "{subject} references no material record at all: {:?}",
                resolved.chosen
            );
            assert!(
                coverage.is_complete(),
                "{subject} loses a material between counting and classifying: {:?}",
                coverage
            );
            assert!(
                coverage.is_fully_unclassified(),
                "{subject} claims a render class that no evidence source asserted: classified {}, \
                 reasons {:?}",
                coverage.classified(),
                coverage.reasons()
            );
            assert_eq!(
                coverage.reasons().get("undeclared"),
                Some(&coverage.total()),
                "{subject}: every refusal must be the missing declaration, not a hidden default"
            );
            assert_eq!(
                coverage.count(MaterialClass::Opaque),
                0,
                "no original material may be counted opaque by default"
            );
            eprintln!(
                "  {}: {} stored materials, {} unclassified, reasons {:?}",
                subject,
                coverage.total(),
                coverage.unclassified(),
                coverage.reasons()
            );
        }
    });
}

/// One subject's resolved row, or the row's reason when it is a hole.
fn resolved(matrix: &ComparisonMatrix, subject: ComparisonSubject) -> &ResolvedSubject {
    let row = matrix
        .row(subject)
        .unwrap_or_else(|| panic!("the set has no row for {subject}"));
    row.resolved().unwrap_or_else(|| {
        panic!(
            "{subject} did not resolve: {}",
            row.reason().unwrap_or("no reason recorded")
        )
    })
}

/// The `gpu` half: every subject's mesh is drawn on a real adapter and comes
/// back as a PNG whose digest matches the bytes on disk.
///
/// A capture that drew nothing is refused by `capture_world_mesh` rather
/// than written, so a uniform frame fails this test instead of producing an
/// artifact that looks like evidence.
#[test]
#[ignore = "requires CS_GAME_DIR and a GPU adapter"]
fn accept_f17_d_gpu_every_subject_draws_a_measured_frame() {
    use cs_app::world::gpu_capture::{CaptureRequest, capture_world_mesh};

    let evidence_dir = workspace_path("private/evidence/F17-D");
    std::fs::create_dir_all(&evidence_dir).expect("private/evidence is writable");

    with_matrix(|matrix| {
        assert!(
            matrix.is_fully_resolved(),
            "the captures follow the resolution; refusals: {:?}",
            matrix
                .rows()
                .iter()
                .filter_map(|row| row.reason().map(str::to_owned))
                .collect::<Vec<_>>()
        );
        for subject in ComparisonSubject::ALL {
            let resolved = resolved(matrix, subject);
            let png = evidence_dir.join(format!("subject-{}.png", subject.code()));
            if png.exists() {
                std::fs::remove_file(&png).expect("an old capture can be replaced");
            }
            let capture = capture_world_mesh(&CaptureRequest {
                group: &resolved.container,
                mesh_index: resolved.chosen.mesh_index,
                render: &resolved.render,
                unknowns: &resolved.unknowns,
                png: &png,
            })
            .unwrap_or_else(|error| panic!("the {subject} subject would not draw: {error}"));

            assert!(
                capture.drew_geometry(),
                "{subject} came back as a blank frame: {capture:?}"
            );
            assert_eq!(
                capture.mesh_index, resolved.chosen.mesh_index,
                "the capture drew the mesh the matrix chose"
            );
            let written = std::fs::read(&png).expect("the PNG is on disk");
            assert_eq!(
                cs_assets::install::sha256(&written),
                capture.png_sha256,
                "{subject}: the digest is of the file on disk"
            );
            eprintln!(
                "  {}: {} drew {} of {} pixels ({} per mille), adapter {}, png {} bytes",
                subject,
                capture.group,
                capture.covered_pixels,
                capture.width * capture.height,
                capture.covered_permille,
                capture.adapter,
                capture.png_bytes,
            );
        }
    });
}
