//! Task #542 acceptance tests: where the pilot's eye sits inside a cockpit —
//! the `cockpit_camera` node the original's vehicle registration resolves,
//! whose stored local translation two camera modes install as the eye.
//!
//! Spec: `specs/F21-cameras-cockpit-views-and-spyglass.md`, stage `### F21-D`
//! (this task is its follow-up). Task test prefix: `accept_t542_`. Findings:
//! `docs/findings/2026-10-11-t542-cockpit-camera-eye-placement.md`.
//!
//! The instruments under test are production code in `cs_app::camera`:
//! [`resolve_cockpit_eye`] measures each audited airframe's own children
//! closure for the name the registration searches for, [`CockpitEyeBinding`]
//! is the bound state a `Declared` eye produces, and
//! [`eye_mechanism_source`] names the owner-supplied image the whole claim
//! stands on. No test here carries its own name search or disassembler: the
//! executable-image test asserts *bytes* — the stored name, the call targets
//! the registration makes, the field loads the translation reader performs
//! and the dispatch-table entries — against the spans the production record
//! names.
//!
//! The suite is split the way the capabilities are:
//!
//! * seven tests read **no** original data and are not ignored: they pin the
//!   contract on authored node arrays — declared, absent, ambiguous and
//!   non-object states, the byte-exact name compare, the wrong-airframe
//!   refusal, the binding a declared eye yields, and the refusals that keep
//!   an eye bound to the archive it was measured in;
//! * one test needs the installation (`#[ignore = "requires CS_GAME_DIR"]`):
//!   all eleven airframes of the real `ZBD/planes.zbd` declare their eye as
//!   exactly one `object3d` `cockpit_camera` with a stored translation;
//! * one test needs the owner-supplied decrypted executable
//!   (`#[ignore = "requires CS_ENGINE_IMAGE"]`): the mechanism's spans must
//!   cover the measured instructions, byte for byte.
//!
//! What no test asserts, because nothing measured it: which body axis each
//! stored component is (the triple is reported in the archive's source
//! units, unlabeled), what the two dispatched camera modes are called, and
//! what `cocky`/`cockz` scripts write at run time — the override channel is
//! measured, its values are runtime state.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use cs_app::camera::coverage::{
    CockpitAirframe, CockpitCoverageReport, CockpitEyeState, audit_cockpit_coverage,
    discover_cockpit_bindings, resolve_cockpit_eye,
};
use cs_app::camera::{
    COCKPIT_EYE_NODE, CockpitCoverageError, CockpitEyeBinding, CockpitEyeError, EyeMechanism,
    eye_mechanism_source,
};
use cs_content::cameras::CockpitBindingSource;
use cs_content::coordinates::EngineImage;
use cs_formats::gamez::{GameZHeader, GameZNodes, NodeKind, RawNode};
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, ContentKind};
use cs_types::evidence::ContentHash;

use crate::coverage::{
    FIXTURE, Node, RETAIL_AIRFRAMES, airframe, archive_id, cockpit_claim, container, decode, line,
    observed, quote, raw_node, retail_cockpit_claim, retail_dir, retail_provenance,
    retail_roster_declarations,
};

// ------------------------------------------------------------- fixtures ---

/// An authored `object3d` node with a stored translation.
fn object_node(spec: &Node, index: u32, translation: [f32; 3]) -> RawNode {
    let mut node = raw_node(spec, index);
    let NodeKind::Object3d(ref mut data) = node.kind else {
        unreachable!("raw_node builds object3d nodes");
    };
    data.flags = cs_formats::gamez::OBJECT3D_FLAGS_TRANSFORMED;
    data.translation = translation;
    node
}

/// An authored node of a non-object3d kind: the translation slot the
/// original's reader addresses does not exist on it.
fn non_object_node(spec: &Node, index: u32, kind: NodeKind) -> RawNode {
    let mut node = raw_node(spec, index);
    node.kind = kind;
    node.info.node_type = kind.tag();
    node
}

/// An authored airframe archive, in the same assembled shape the coverage
/// suite's fixture builds.
fn eye_archive(nodes: Vec<RawNode>) -> GameZNodes {
    GameZNodes {
        header: GameZHeader {
            signature: cs_formats::zbd::GAMEZ_SIGNATURE,
            version: cs_formats::zbd::GAMEZ_VERSION,
            unk08: 0,
            texture_count: 0,
            textures_offset: cs_formats::gamez::GAMEZ_HEADER_BYTES as u32,
            materials_offset: 0,
            meshes_offset: 0,
            node_array_size: nodes.len() as u32,
            light_index: 0,
            nodes_offset: 0,
        },
        nodes,
        info_offset: 0,
        info_end: 0,
        data_offset: 0,
        data_end: 0,
        findings: Vec::new(),
    }
}

/// One authored `gungauge` binding for the airframe var the shared claim
/// carries — the coverage report the eye pass takes as its airframe list.
fn coverage_over(
    archive: &GameZNodes,
    airframes: &[CockpitAirframe],
    archive_id: &ContentId,
) -> CockpitCoverageReport {
    let bytes = container(&[(
        b"support\\cockpit.gw",
        vec![
            line(&[b"FindNode", b"%player_plane%"]),
            line(&[b"FindSubNode", b"gungauge"]),
        ],
    )]);
    let discovery =
        discover_cockpit_bindings(&decode(&bytes), &cockpit_claim(), FIXTURE, observed(0, 16))
            .expect("the authored container declares a binding");
    audit_cockpit_coverage(&discovery, airframes, archive, archive_id)
        .expect("every declared root exists in the authored archive")
}

/// The two-airframe roster the authored archives hold.
fn two_airframes() -> Vec<CockpitAirframe> {
    vec![
        airframe("synthetic.kestrel", "player_kestrel"),
        airframe("synthetic.warhawk", "player_warhawk"),
    ]
}

/// The authored archive every eye test starts from: `player_kestrel` (0)
/// holding `geometry` (1) → `markers` (2) → `cockpit_camera` (3), and
/// `player_warhawk` (4) holding only a `gungauge` (5) — so the kestrel
/// declares its eye and the warhawk does not, and no search can leave the
/// airframe it started from to find one.
fn eye_fixture() -> GameZNodes {
    eye_archive(vec![
        raw_node(
            &Node {
                name: "player_kestrel",
                parent: None,
                children: vec![1],
                mesh_index: -1,
            },
            0,
        ),
        raw_node(
            &Node {
                name: "geometry",
                parent: Some(0),
                children: vec![2],
                mesh_index: -1,
            },
            1,
        ),
        raw_node(
            &Node {
                name: "markers",
                parent: Some(1),
                children: vec![3],
                mesh_index: -1,
            },
            2,
        ),
        object_node(
            &Node {
                name: COCKPIT_EYE_NODE,
                parent: Some(2),
                children: Vec::new(),
                mesh_index: -1,
            },
            3,
            [0.5, 1.25, -2.0],
        ),
        raw_node(
            &Node {
                name: "player_warhawk",
                parent: None,
                children: vec![5],
                mesh_index: -1,
            },
            4,
        ),
        raw_node(
            &Node {
                name: "gungauge",
                parent: Some(4),
                children: Vec::new(),
                mesh_index: 41,
            },
            5,
        ),
    ])
}

// ------------------------------------------------- the resolution states ---

/// The eye is the `cockpit_camera` node's stored local translation, measured
/// inside the airframe's own subtree: the authored `0.5 / 1.25 / -2.0` comes
/// back verbatim, in source units, with the node's own index.
#[test]
fn accept_t542_the_eye_is_the_cockpit_camera_nodes_stored_local_translation() {
    let archive = eye_fixture();
    let coverage = coverage_over(&archive, &two_airframes(), &archive_id());
    let report =
        resolve_cockpit_eye(&coverage, &archive, &archive_id()).expect("the archive resolves");

    assert_eq!(report.rows().len(), 2);
    let row = report
        .row("airframe/synthetic.kestrel")
        .expect("the kestrel has a row");
    assert_eq!(
        row.state(),
        &CockpitEyeState::Declared {
            node_index: 3,
            translation: [0.5, 1.25, -2.0],
        },
        "the stored triple is the eye, bit for bit — not derived, not re-anchored"
    );
    assert!(row.state().is_declared());
    assert_eq!(row.state().label(), "declared");
    assert_eq!(row.state().node_index(), Some(3));
    assert_eq!(row.state().translation(), Some([0.5, 1.25, -2.0]));
}

/// A node spelled `Cockpit_Camera` is **not** the eye, because the measured
/// lookup compares bytes exactly — an audit that lowercased would resolve an
/// eye the original's own registration would never find.
#[test]
fn accept_t542_the_eye_lookup_is_byte_exact_because_the_measured_search_is() {
    let archive = eye_archive(vec![
        raw_node(
            &Node {
                name: "player_kestrel",
                parent: None,
                children: vec![1],
                mesh_index: -1,
            },
            0,
        ),
        object_node(
            &Node {
                name: "Cockpit_Camera",
                parent: Some(0),
                children: Vec::new(),
                mesh_index: -1,
            },
            1,
            [0.5, 1.25, -2.0],
        ),
    ]);
    let coverage = coverage_over(&archive, &two_airframes()[..1], &archive_id());
    let report =
        resolve_cockpit_eye(&coverage, &archive, &archive_id()).expect("the archive resolves");
    assert_eq!(
        report.rows()[0].state(),
        &CockpitEyeState::Absent,
        "the case-variant name is absent, exactly as the original's compare reports it"
    );
}

/// Two `cockpit_camera` nodes in one airframe stay `Ambiguous` with both
/// indices, in array order — the original's last-child-first search is a
/// finding about the corpus, not a tiebreaker this pass may apply.
#[test]
fn accept_t542_two_eye_nodes_in_one_airframe_are_ambiguous_not_resolved_by_position() {
    let archive = eye_archive(vec![
        raw_node(
            &Node {
                name: "player_kestrel",
                parent: None,
                children: vec![1, 2],
                mesh_index: -1,
            },
            0,
        ),
        object_node(
            &Node {
                name: COCKPIT_EYE_NODE,
                parent: Some(0),
                children: Vec::new(),
                mesh_index: -1,
            },
            1,
            [0.5, 1.25, -2.0],
        ),
        object_node(
            &Node {
                name: COCKPIT_EYE_NODE,
                parent: Some(0),
                children: Vec::new(),
                mesh_index: -1,
            },
            2,
            [9.0, 9.0, 9.0],
        ),
    ]);
    let coverage = coverage_over(&archive, &two_airframes()[..1], &archive_id());
    let report =
        resolve_cockpit_eye(&coverage, &archive, &archive_id()).expect("the archive resolves");
    let state = report.rows()[0].state();
    assert_eq!(
        state,
        &CockpitEyeState::Ambiguous {
            node_indices: vec![1, 2]
        },
        "both candidates reported, in array order — never the later one silently"
    );
    assert_eq!(state.node_index(), None);
    assert_eq!(state.translation(), None);
    assert_eq!(
        CockpitEyeBinding::try_new(state),
        Err(CockpitEyeError::NotDeclared),
        "an ambiguous eye has no stored translation to bind"
    );
}

/// A `cockpit_camera` that is not an `object3d` reports the kind it actually
/// is, because the slot the original's translation reader addresses only
/// exists on an object record.
#[test]
fn accept_t542_an_eye_node_that_is_not_an_object3d_reports_its_kind() {
    let archive = eye_archive(vec![
        raw_node(
            &Node {
                name: "player_kestrel",
                parent: None,
                children: vec![1],
                mesh_index: -1,
            },
            0,
        ),
        non_object_node(
            &Node {
                name: COCKPIT_EYE_NODE,
                parent: Some(0),
                children: Vec::new(),
                mesh_index: -1,
            },
            1,
            NodeKind::Camera,
        ),
    ]);
    let coverage = coverage_over(&archive, &two_airframes()[..1], &archive_id());
    let report =
        resolve_cockpit_eye(&coverage, &archive, &archive_id()).expect("the archive resolves");
    let state = report.rows()[0].state();
    assert_eq!(
        state,
        &CockpitEyeState::NotAnObject {
            node_index: 1,
            kind: cs_formats::gamez::NODE_TYPE_CAMERA,
        },
        "the node's stored kind is reported, not an invented translation"
    );
    assert_eq!(state.node_index(), Some(1));
    assert_eq!(state.translation(), None);
}

/// The search is each airframe's own children closure: a `cockpit_camera`
/// under the other audited airframe, and a third under no airframe at all,
/// are both invisible to the row that does not own them.
#[test]
fn accept_t542_the_eye_search_never_leaves_the_airframe_it_started_from() {
    let archive = eye_archive(vec![
        raw_node(
            &Node {
                name: "player_kestrel",
                parent: None,
                children: vec![1],
                mesh_index: -1,
            },
            0,
        ),
        raw_node(
            &Node {
                name: "gungauge",
                parent: Some(0),
                children: Vec::new(),
                mesh_index: 41,
            },
            1,
        ),
        raw_node(
            &Node {
                name: "player_warhawk",
                parent: None,
                children: vec![3],
                mesh_index: -1,
            },
            2,
        ),
        object_node(
            &Node {
                name: COCKPIT_EYE_NODE,
                parent: Some(2),
                children: Vec::new(),
                mesh_index: -1,
            },
            3,
            [9.0, 9.0, 9.0],
        ),
        object_node(
            &Node {
                name: COCKPIT_EYE_NODE,
                parent: None,
                children: Vec::new(),
                mesh_index: -1,
            },
            4,
            [7.0, 7.0, 7.0],
        ),
    ]);
    let coverage = coverage_over(&archive, &two_airframes(), &archive_id());
    let report =
        resolve_cockpit_eye(&coverage, &archive, &archive_id()).expect("the archive resolves");

    assert_eq!(
        report
            .row("airframe/synthetic.kestrel")
            .expect("the kestrel has a row")
            .state(),
        &CockpitEyeState::Absent,
        "the warhawk's cockpit_camera is the warhawk's, and the unparented one \
         is no airframe's: neither is attributed to the kestrel"
    );
    assert_eq!(
        report
            .row("airframe/synthetic.warhawk")
            .expect("the warhawk has a row")
            .state(),
        &CockpitEyeState::Declared {
            node_index: 3,
            translation: [9.0, 9.0, 9.0],
        },
        "and the warhawk's own eye is its own subtree's, not the stray one's"
    );
}

/// A `Declared` eye binds the model node the original's own registration
/// resolves — `cockpit_camera` — carrying the node's index and its stored
/// translation for a `CockpitViewpoint::source`; every other state refuses.
#[test]
fn accept_t542_a_declared_eye_binds_the_model_node_the_registration_resolves() {
    let declared = CockpitEyeState::Declared {
        node_index: 3,
        translation: [0.5, 1.25, -2.0],
    };
    let binding = CockpitEyeBinding::try_new(&declared).expect("a declared eye binds");
    assert_eq!(
        binding.source(),
        &CockpitBindingSource::ModelNode {
            node: COCKPIT_EYE_NODE.to_owned()
        },
        "the binding names the node the registration searches for"
    );
    assert_eq!(binding.node_index(), 3);
    assert_eq!(
        binding.translation(),
        [0.5, 1.25, -2.0],
        "the stored triple passes through unlabeled, in source units"
    );
    assert_eq!(binding.state(), &declared);

    for state in [
        CockpitEyeState::Absent,
        CockpitEyeState::Ambiguous {
            node_indices: vec![1, 2],
        },
        CockpitEyeState::NotAnObject {
            node_index: 1,
            kind: cs_formats::gamez::NODE_TYPE_CAMERA,
        },
    ] {
        assert_eq!(
            CockpitEyeBinding::try_new(&state),
            Err(CockpitEyeError::NotDeclared),
            "{state:?} has no stored translation to install as the eye"
        );
    }
}

/// A report resolved over a different archive is refused twice over: a row
/// audited under another container id is a `ContainerMismatch`, and a node
/// array that does not even hold the row's root is `ResolvedNodeAbsent` —
/// the eye pass never reports one archive's eye against another's bytes.
#[test]
fn accept_t542_an_eye_pass_over_a_different_archive_is_refused() {
    let archive = eye_fixture();
    let coverage = coverage_over(&archive, &two_airframes()[..1], &archive_id());

    let other_id =
        ContentId::from_source(ContentKind::InstallFile, "zbd_2f_other.zbd").expect("a valid id");
    assert_eq!(
        resolve_cockpit_eye(&coverage, &archive, &other_id),
        Err(CockpitCoverageError::ContainerMismatch {
            airframe: "airframe/synthetic.kestrel".to_owned(),
            declared: archive_id().as_str().to_owned(),
            measured: other_id.as_str().to_owned(),
        }),
        "a row audited over another archive must not pick up this one's eye"
    );

    // Same id, an empty node array: the report was measured over six nodes
    // and this array holds none of them, so the row's root itself is absent.
    let short = eye_archive(Vec::new());
    let coverage = coverage_over(&archive, &two_airframes()[..1], &archive_id());
    assert_eq!(
        resolve_cockpit_eye(&coverage, &short, &archive_id()),
        Err(CockpitCoverageError::ResolvedNodeAbsent {
            airframe: "airframe/synthetic.kestrel".to_owned(),
            node: "player_kestrel".to_owned(),
            node_index: 0,
        }),
        "an eye reported against an array without the root would be another \
         container's eye"
    );
}

// -------------------------------------------------------- the retail pass ---

/// The measured corpus: all eleven airframes of the real `ZBD/planes.zbd`
/// declare their eye — each as exactly one `object3d` node named
/// `cockpit_camera`, inside its own subtree, holding a stored translation —
/// which is what the original's own registration installs, and what the
/// loading-script bindings could never have said.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t542_retail_every_airframe_declares_its_eye_as_one_stored_translation() {
    use cs_content::scene::discover_airframe_roster;
    use cs_formats::gamez::read_gamez_nodes;
    use cs_formats::interp::decode_interp;
    use cs_formats::io::ParseContext;

    let game_dir = retail_dir();
    let discovery = cs_assets::install::discover(&game_dir).expect("production discovery reads it");
    let install_sha256 = cs_assets::install::fingerprint(&discovery.manifest);
    let container_label = "ZBD/interp.zbd";
    let bytes = std::fs::read(game_dir.join("ZBD").join("interp.zbd"))
        .expect("the loading-script container must be there");
    let decoded = decode_interp(&mut ParseContext::with_defaults(container_label), &bytes)
        .expect("the real container decodes");
    let bindings = discover_cockpit_bindings(
        &decoded,
        &retail_cockpit_claim(install_sha256),
        container_label,
        retail_provenance(install_sha256, container_label, 12_850, 5_713),
    )
    .expect("the claimed script is the one the container holds");

    let roster = discover_airframe_roster(&decoded, &retail_roster_declarations(install_sha256))
        .expect("the real roster discovery reports findings, not a contradiction");
    assert_eq!(roster.airframe_count(), RETAIL_AIRFRAMES);
    let airframes: Vec<CockpitAirframe> = roster
        .discovered()
        .iter()
        .map(CockpitAirframe::from)
        .collect();

    let archive_label = "ZBD/planes.zbd";
    let archive_id = ContentId::from_source(
        ContentKind::InstallFile,
        &cs_content::catalog::baseline::install_file_key(archive_label),
    )
    .expect("the normalized install-file key is a valid id key");
    let planes = std::fs::read(game_dir.join("ZBD").join("planes.zbd"))
        .expect("the shared airframe archive must be there");
    let declared_archive = discovery
        .manifest
        .files
        .iter()
        .find(|row| row.relative_spelling.logical_key() == "zbd/planes.zbd")
        .map(|row| row.sha256)
        .expect("the airframe archive is inventoried");
    assert_eq!(
        cs_assets::install::sha256(&planes),
        declared_archive,
        "the node array the eye pass walks is the array the installation \
         manifest declares for zbd/planes.zbd"
    );
    let nodes = read_gamez_nodes(&mut ParseContext::with_defaults(archive_label), &planes)
        .expect("the airframe archive's node array reads");

    let coverage = audit_cockpit_coverage(&bindings, &airframes, &nodes, &archive_id)
        .expect("every declared root exists in the real archive");
    let report = resolve_cockpit_eye(&coverage, &nodes, &archive_id)
        .expect("the eye pass resolves over the archive the audit measured");

    assert_eq!(report.archive(), archive_id.as_str());
    assert_eq!(report.rows().len(), RETAIL_AIRFRAMES);
    assert_eq!(
        report.declared(),
        RETAIL_AIRFRAMES,
        "every audited airframe declares its eye — the task's headline measurement"
    );
    assert_eq!(report.undeclared().count(), 0);
    assert!(report.is_complete());

    // Every declaration is the archive's own node: an object3d record named
    // `cockpit_camera` byte for byte, inside that airframe's subtree and
    // nowhere else — twenty-two such nodes exist in the whole archive (the
    // other eleven belong to airframes the roster does not declare), and
    // each audited row names its own index, all distinct.
    let mut indices = BTreeSet::new();
    let mut translations = BTreeSet::new();
    for row in report.rows() {
        let CockpitEyeState::Declared {
            node_index,
            translation,
        } = row.state()
        else {
            panic!("{} does not declare its eye", row.airframe());
        };
        let node = nodes
            .get(*node_index)
            .expect("the resolved node is in the array");
        assert_eq!(
            node.name,
            COCKPIT_EYE_NODE,
            "{}: the eye node carries the stored name byte for byte",
            row.airframe()
        );
        assert!(
            node.object3d().is_some(),
            "{}: the eye node is an object3d, kind {}",
            row.airframe(),
            node.kind.label()
        );
        assert_ne!(
            *node_index,
            row.root_index(),
            "{}: the eye is a member of the subtree, never the root itself",
            row.airframe()
        );
        assert!(
            node.info.mesh_index < 0,
            "{}: the eye node is a marker, not geometry",
            row.airframe()
        );
        assert!(
            translation.iter().all(|component| component.is_finite()),
            "{}: the stored eye is finite: {translation:?}",
            row.airframe()
        );
        assert!(
            indices.insert(*node_index),
            "two airframes resolved the same eye node {node_index}"
        );
        translations.insert(translation.map(f32::to_bits));

        // The binding the declared state yields is the model node the
        // original's registration resolves, carrying the same stored triple.
        let binding = CockpitEyeBinding::try_new(row.state())
            .expect("a declared eye binds the measured node");
        assert_eq!(
            binding.source(),
            &CockpitBindingSource::ModelNode {
                node: COCKPIT_EYE_NODE.to_owned()
            }
        );
        assert_eq!(binding.node_index(), *node_index);
        assert_eq!(binding.translation(), *translation);
    }
    assert!(
        translations.len() > 1,
        "the eyes are authored per airframe, not one shared constant"
    );

    // The archive holds `cockpit_camera` nodes the roster never audits —
    // counted here so the eleven above are visibly the airframes' own and
    // not "every node of that name": each of the eleven AI-variant roots
    // stores one too, which the same registration resolves when an AI
    // vehicle is the view's subject.
    let all_eye_nodes = nodes
        .nodes
        .iter()
        .filter(|node| node.name.as_bytes() == COCKPIT_EYE_NODE.as_bytes())
        .count();
    assert_eq!(
        all_eye_nodes, 22,
        "the whole archive stores twenty-two eye nodes — one under every \
         player and AI airframe root; the eleven audited are each inside \
         their own airframe's subtree"
    );
    eprintln!("T542 retail: {RETAIL_AIRFRAMES}/{RETAIL_AIRFRAMES} airframes declare their eye");
    for row in report.rows() {
        let CockpitEyeState::Declared {
            node_index,
            translation,
        } = row.state()
        else {
            unreachable!()
        };
        eprintln!(
            "T542 retail: {} eye at node {node_index} = {translation:?}",
            row.airframe()
        );
    }

    write_eye_census(
        &report,
        &nodes,
        install_sha256,
        cs_assets::install::content_fingerprint(&discovery.manifest),
        declared_archive,
    );
}

// ------------------------------------------------- the measured image -----

/// The mechanism's spans cover the measured bytes: the stored
/// `cockpit_camera` name, the recursive search's own self-call, the
/// registration's push of that very name's address and its call list — seven
/// name searches and the one translation read — the reader's
/// `+0x54`/`+0x58`/`+0x5c` field loads, the bounded mode dispatch and the two
/// table entries that route to the transform, the transform's own load of
/// the three eye globals, and the `cocky`/`cockz` override branches.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_t542_image_the_mechanism_spans_cover_the_measured_instructions() {
    use cs_app::camera::eye::{
        EYE_APPLY_LENGTH, EYE_APPLY_OFFSET, EYE_DISPATCH_LENGTH, EYE_DISPATCH_OFFSET, EYE_IMAGE,
        EYE_LOOKUP_LENGTH, EYE_LOOKUP_OFFSET, EYE_NAME_LENGTH, EYE_NAME_OFFSET,
        EYE_OVERRIDE_LENGTH, EYE_OVERRIDE_NAMES_LENGTH, EYE_OVERRIDE_NAMES_OFFSET,
        EYE_OVERRIDE_OFFSET, EYE_READ_LENGTH, EYE_READ_OFFSET, EYE_REGISTER_LENGTH,
        EYE_REGISTER_OFFSET, EYE_TABLE_LENGTH, EYE_TABLE_OFFSET, EYE_THUNK_LENGTH,
        EYE_THUNK_OFFSET,
    };

    let image = cs_content::coordinates::load_engine_image()
        .expect("CS_ENGINE_IMAGE names the owner-supplied decrypted executable");
    assert_eq!(
        image.digest,
        cs_content::coordinates::original_image_digest(),
        "the loader's own check already binds the spans to the measured bytes"
    );

    let mechanism = eye_mechanism_source(&image).expect("the measured image yields its spans");
    let spans = [
        ("name", &mechanism.name, EYE_NAME_OFFSET, EYE_NAME_LENGTH),
        (
            "lookup",
            &mechanism.lookup,
            EYE_LOOKUP_OFFSET,
            EYE_LOOKUP_LENGTH,
        ),
        (
            "register",
            &mechanism.register,
            EYE_REGISTER_OFFSET,
            EYE_REGISTER_LENGTH,
        ),
        ("read", &mechanism.read, EYE_READ_OFFSET, EYE_READ_LENGTH),
        (
            "dispatch",
            &mechanism.dispatch,
            EYE_DISPATCH_OFFSET,
            EYE_DISPATCH_LENGTH,
        ),
        (
            "dispatch_table",
            &mechanism.dispatch_table,
            EYE_TABLE_OFFSET,
            EYE_TABLE_LENGTH,
        ),
        (
            "thunk",
            &mechanism.thunk,
            EYE_THUNK_OFFSET,
            EYE_THUNK_LENGTH,
        ),
        (
            "apply",
            &mechanism.apply,
            EYE_APPLY_OFFSET,
            EYE_APPLY_LENGTH,
        ),
        (
            "override_names",
            &mechanism.override_names,
            EYE_OVERRIDE_NAMES_OFFSET,
            EYE_OVERRIDE_NAMES_LENGTH,
        ),
        (
            "override",
            &mechanism.override_,
            EYE_OVERRIDE_OFFSET,
            EYE_OVERRIDE_LENGTH,
        ),
    ];
    for (name, span, offset, length) in &spans {
        assert_eq!(
            span.install_sha256(),
            image.digest,
            "{name}: the span names the measured image"
        );
        assert_eq!(span.container_path(), EYE_IMAGE, "{name}");
        assert_eq!(span.member_key(), None, "{name}: the image is the source");
        assert_eq!(
            (span.offset(), span.length()),
            (*offset, *length),
            "{name}: the span names the measured bytes"
        );
    }
    let bytes = |span: &SourceSpan| {
        &image.bytes[span.offset() as usize..(span.offset() + span.length()) as usize]
    };

    // The stored node name, at the span the record names.
    assert_eq!(
        bytes(&mechanism.name),
        b"cockpit_camera\0",
        "the registration's matched name, stored"
    );

    // The lookup is recursive: the only `e8 rel32` call inside its span
    // resolves to the lookup's own virtual address (.text = image base +
    // file offset here).
    let lookup = bytes(&mechanism.lookup);
    let lookup_va = 0x40_0000_u64 + mechanism.lookup.offset();
    let lookup_calls: Vec<u64> = (0..lookup.len().saturating_sub(5))
        .filter(|&index| lookup[index] == 0xe8)
        .map(|index| {
            let relative =
                i32::from_le_bytes(lookup[index + 1..index + 5].try_into().expect("four bytes"));
            (lookup_va + index as u64 + 5).wrapping_add_signed(relative as i64)
        })
        .collect();
    assert_eq!(
        lookup_calls,
        vec![lookup_va],
        "the child-by-name search recurses on itself, and nothing else"
    );

    // The registration pushes the *name span's own virtual address* — the
    // `name` span and the `register` span are linked by the instruction, not
    // by a comment.
    let register = bytes(&mechanism.register);
    let name_va = 0x40_0000_u32 + mechanism.name.offset() as u32;
    let pushed = u32::from_le_bytes(
        register[0x81..0x85]
            .try_into()
            .expect("the push operand is four bytes"),
    );
    assert_eq!(
        pushed, name_va,
        "the registration resolves the name the name span holds"
    );
    // Its whole call list: the recursive name search seven times (healthy,
    // dontmove, cockpit1, cockpit_camera, structure, geometry, markers) and
    // the object3d translate reader once.
    let register_va = 0x40_0000_u64 + mechanism.register.offset();
    let mut register_calls: Vec<u64> = (0..register.len().saturating_sub(5))
        .filter(|&index| register[index] == 0xe8)
        .map(|index| {
            let relative = i32::from_le_bytes(
                register[index + 1..index + 5]
                    .try_into()
                    .expect("four bytes"),
            );
            (register_va + index as u64 + 5).wrapping_add_signed(relative as i64)
        })
        .collect();
    register_calls.sort_unstable();
    let read_va = 0x40_0000_u64 + mechanism.read.offset();
    assert_eq!(
        register_calls,
        [vec![read_va], vec![lookup_va; 7]].concat(),
        "the registration's own calls: seven name searches, one translate read"
    );

    // The translate reader's three field loads: +0x54, +0x58, +0x5c — the
    // local translation slot of the object3d record.
    let read = bytes(&mechanism.read);
    for (index, field) in [(0x6a_usize, 0x54_u8), (0x73, 0x58), (0x7c, 0x5c)] {
        assert_eq!(
            &read[index..index + 3],
            &[0x8b, 0x41, field],
            "the reader loads the +{field:#x} component of the node record"
        );
    }

    // The dispatch: mode index at [cam+0x14c], bounded to ten, then the
    // table jump — the whole measured instruction sequence, byte for byte.
    assert_eq!(
        bytes(&mechanism.dispatch),
        &[
            0xa1, 0x78, 0xef, 0x64, 0x00, 0x8b, 0x80, 0x4c, 0x01, 0x00, 0x00, 0x83, 0xf8, 0x09,
            0x77, 0x74, 0xff, 0x24, 0x85, 0x48, 0xc6, 0x42, 0x00
        ],
        "load mode, bounds-check to ten, jump through the table at 0x42c648"
    );

    // The table's ten entries: entries 6 and 7 hold the thunk that
    // tail-calls the eye transform — the two camera modes the eye serves.
    let thunk_va = 0x40_0000_u32 + mechanism.thunk.offset() as u32;
    let table: Vec<u32> = bytes(&mechanism.dispatch_table)
        .as_chunks::<4>()
        .0
        .iter()
        .map(|word| u32::from_le_bytes(*word))
        .collect();
    assert_eq!(table.len(), 10);
    let eye_entries: Vec<usize> = table
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| (*entry == thunk_va).then_some(index))
        .collect();
    assert_eq!(
        eye_entries,
        vec![6, 7],
        "camera modes 6 and 7 dispatch to the eye transform"
    );

    // The thunk tail-calls the apply routine — its one `e8 rel32` (opcode at
    // index 5, operand at 6..10) lands on the apply span's own virtual
    // address.
    let thunk = bytes(&mechanism.thunk);
    let relative = i32::from_le_bytes(thunk[6..10].try_into().expect("four bytes"));
    let apply_va = 0x40_0000_u64 + mechanism.apply.offset();
    assert_eq!(
        (thunk_va as u64 + 5 + 5).wrapping_add_signed(relative as i64),
        apply_va,
        "the table's eye entries reach the transform through the thunk"
    );

    // The transform's head loads the three eye globals verbatim —
    // `mov eax,[0x71c328]; mov ecx,[0x71c32c]; mov edx,[0x71c330]` — then
    // addresses the aircraft world matrix at [edi+0x180].
    let apply = bytes(&mechanism.apply);
    assert_eq!(
        &apply[0x9..0x1a],
        &[
            0xa1, 0x28, 0xc3, 0x71, 0x00, 0x8b, 0x0d, 0x2c, 0xc3, 0x71, 0x00, 0x8b, 0x15, 0x30,
            0xc3, 0x71, 0x00
        ],
        "the registered eye triple is the transform's input"
    );
    assert_eq!(
        &apply[0x20..0x26],
        &[0x8d, 0xb7, 0x80, 0x01, 0x00, 0x00],
        "the aircraft's world matrix at [edi+0x180] transforms it"
    );

    // The override channel: the two variable names and the two compare-and-
    // store branches that write the second and third eye globals.
    assert_eq!(
        bytes(&mechanism.override_names),
        b"cocky\0\0\0cockz\0",
        "the two script-variable names, stored together"
    );
    let override_ = bytes(&mechanism.override_);
    for (index, constant) in [(0xb, 0x622ebc_u32), (0x4e, 0x622ec4_u32)] {
        assert_eq!(
            &override_[index..index + 5],
            &[
                0xbf,
                constant.to_le_bytes()[0],
                constant.to_le_bytes()[1],
                constant.to_le_bytes()[2],
                constant.to_le_bytes()[3]
            ],
            "the branch compares against the {constant:#x} variable name"
        );
    }
    for (index, global) in [(0x2a, 0x71c32c_u32), (0x6d, 0x71c330_u32)] {
        assert_eq!(
            &override_[index..index + 6],
            &[
                0xd9,
                0x1d,
                global.to_le_bytes()[0],
                global.to_le_bytes()[1],
                global.to_le_bytes()[2],
                global.to_le_bytes()[3]
            ],
            "and writes the {global:#x} eye component"
        );
    }

    write_mechanism_census(&mechanism, &image, &eye_entries, &register_calls);
}

// ------------------------------------------------- the evidence artifacts ---

/// The private evidence directory the artifacts share with the harness, or
/// `None` outside an evidence run — the tests assert the same measurements
/// either way.
fn evidence_dir() -> Option<PathBuf> {
    let dir = std::env::var("CS_EVIDENCE_DIR").ok()?;
    let root = git(&["rev-parse", "--show-toplevel"]);
    let directory = if Path::new(&dir).is_absolute() {
        PathBuf::from(&dir)
    } else {
        Path::new(&root).join(&dir)
    };
    std::fs::create_dir_all(&directory).expect("the evidence directory is writable");
    Some(directory)
}

/// One git query at the workspace root, trimmed.
fn git(args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// Writes the measured eye census under `CS_EVIDENCE_DIR` when the evidence
/// run sets it, so the acceptance report carries this run's own rows instead
/// of only the log lines they are asserted on.
///
/// What it holds is **derived** data: airframe ids, node indices, the stored
/// name and the stored translation triples — the same numbers the log
/// asserts. No original display text, no geometry, no decompiled code. The
/// file carries the candidate tree it was produced from so a census left
/// over from an earlier commit is refused rather than reused.
fn write_eye_census(
    report: &cs_app::camera::coverage::CockpitEyeReport,
    nodes: &GameZNodes,
    install_sha256: ContentHash,
    content_sha256: ContentHash,
    archive_sha256: ContentHash,
) {
    let Some(directory) = evidence_dir() else {
        return;
    };
    let rows: Vec<String> = report
        .rows()
        .iter()
        .map(|row| {
            let CockpitEyeState::Declared {
                node_index,
                translation,
            } = row.state()
            else {
                panic!("a census row outside an evidence run's measured corpus")
            };
            let node = nodes.get(*node_index).expect("the node is in the array");
            format!(
                "{{\"airframe\":{},\"root\":{},\"root_index\":{},\"node_index\":{},\
                 \"name\":{},\"translation\":[{:?},{:?},{:?}]}}",
                quote(row.airframe()),
                quote(row.root()),
                row.root_index(),
                node_index,
                quote(&node.name),
                translation[0],
                translation[1],
                translation[2],
            )
        })
        .collect();
    let document = format!(
        "{{\"schema\":\"cs-t542-eye-placement/1\",\"candidate_tree\":{},\
         \"install_sha256\":{},\"content_sha256\":{},\"archive_sha256\":{},\
         \"archive\":{},\"declared\":{},\"airframes\":[{}]}}\n",
        quote(&git(&["rev-parse", "HEAD^{tree}"])),
        quote(&install_sha256.to_hex()),
        quote(&content_sha256.to_hex()),
        quote(&archive_sha256.to_hex()),
        quote(report.archive()),
        report.declared(),
        rows.join(","),
    );
    let path = directory.join("eye-placement.json");
    std::fs::write(&path, &document)
        .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
    println!("wrote {}", path.display());
}

/// Writes the mechanism's measured image record under `CS_EVIDENCE_DIR` when
/// the evidence run sets it: the image's own digest, the ten spans the
/// production record names, the registration's decoded call targets and the
/// dispatch-table entries that route to the eye transform.
///
/// Everything in it is a number or a digest — the instruction bytes stay in
/// the owner-supplied image, never in Git.
fn write_mechanism_census(
    mechanism: &EyeMechanism,
    image: &EngineImage,
    eye_entries: &[usize],
    register_call_targets: &[u64],
) {
    let Some(directory) = evidence_dir() else {
        return;
    };
    let span = |name: &str, span: &SourceSpan| {
        format!(
            "\"{name}\":{{\"offset\":{},\"length\":{}}}",
            span.offset(),
            span.length()
        )
    };
    let entries: Vec<String> = eye_entries.iter().map(usize::to_string).collect();
    let targets: Vec<String> = register_call_targets.iter().map(u64::to_string).collect();
    let document = format!(
        "{{\"schema\":\"cs-t542-eye-mechanism/1\",\"candidate_tree\":{},\
         \"image_sha256\":{},{},{},{},{},{},{},{},{},{},{},\"eye_entries\":[{}],\
         \"register_call_targets\":[{}]}}\n",
        quote(&git(&["rev-parse", "HEAD^{tree}"])),
        quote(&image.digest.to_hex()),
        span("name", &mechanism.name),
        span("lookup", &mechanism.lookup),
        span("register", &mechanism.register),
        span("read", &mechanism.read),
        span("dispatch", &mechanism.dispatch),
        span("dispatch_table", &mechanism.dispatch_table),
        span("thunk", &mechanism.thunk),
        span("apply", &mechanism.apply),
        span("override_names", &mechanism.override_names),
        span("override", &mechanism.override_),
        entries.join(","),
        targets.join(","),
    );
    let path = directory.join("eye-mechanism.json");
    std::fs::write(&path, &document)
        .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
    println!("wrote {}", path.display());
}
