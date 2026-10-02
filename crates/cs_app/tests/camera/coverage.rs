//! F21-D acceptance tests: what the installation declares about the pilot's
//! view, measured against this camera contract.
//!
//! Spec: `specs/F21-cameras-cockpit-views-and-spyglass.md`, stage `### F21-D`,
//! minimum scenario **AC04** — *"Match original cockpit/view behaviors with
//! recorded input and captures"*. Task test prefix: `accept_f21_d_`.
//!
//! The instrument under test is production code in `cs_app::camera::coverage`:
//! [`discover_view_controls`] censuses the camera commands a loading-script
//! container declares, [`discover_cockpit_bindings`] walks the script that binds
//! an aircraft's cockpit nodes, and [`audit_cockpit_coverage`] checks that
//! binding set against the real node array of the airframe archive. No test here
//! carries its own script walk, its own subtree search or its own coverage
//! judgement.
//!
//! The suite is split the way the capabilities are:
//!
//! * six tests read **no** original data and are not ignored, so CI runs them.
//!   They pin the contract on authored containers: the census shape, the walk
//!   order, the refusals, the per-airframe resolution and the fact that the
//!   pilot's eye stays undeclared however complete the bindings are;
//! * three tests need the installation (`#[ignore = "requires CS_GAME_DIR"]`):
//!   the real camera-command census, the real cockpit binding set checked
//!   against the real `ZBD/planes.zbd` node array, and a real capture of one
//!   bound cockpit mesh on a real adapter.
//!
//! AC04's comparison against the original *running* is **not** what these tests
//! do and cannot be: no default key binding ships in any readable file (F22-H),
//! and no original run has been supplied. What is measured here is what the
//! original's own files declare, which is the half that files can answer; the
//! rest is recorded as an unknown in the stage's finding and filed as a task.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use cs_app::camera::coverage::{
    AirframeCockpitCoverage, CameraOperation, CockpitAirframe, CockpitBindingClaim,
    CockpitCoverageError, CockpitEyeCoverage, CockpitFinding, CockpitNodeCoverage,
    ViewCommandClaim, ViewControlCoverage, ViewControlEffect, ViewControlError, ViewControlFinding,
    audit_cockpit_coverage, discover_cockpit_bindings, discover_view_controls, eye_placement,
};
use cs_content::cameras::CockpitBindingSource;
use cs_content::mesh::RenderMesh;
use cs_content::scene::{AirframeDeclaration, RosterDeclarations, RosterRoleRule};
use cs_formats::gamez::{PrimitiveKind, RawCorner, RawMesh};
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, ContentKind, Provenance};
use cs_types::evidence::{ClaimStatus, ContentHash};

// ------------------------------------------------------------- fixtures ---

/// The claim id every authored fixture value is recorded under.
fn claim() -> cs_types::evidence::ClaimId {
    cs_types::evidence::ClaimId::new("f21d.coverage-fixture").expect("a valid claim id")
}

/// The container label the authored fixtures carry.
const FIXTURE: &str = "synthetic/f21d.interp";

/// The archive the authored airframes live in, as the installation spells it.
///
/// The **key** an install-file id carries is the production normalizer's
/// (`cs_content::catalog::baseline::install_file_key`), not this string: a
/// `ContentId` key accepts no `/` or `\\`, and the roster discovery a retail
/// caller uses derives its container keys through that same normalizer, so a
/// fixture that spelled its own would never match a real row.
const ARCHIVE_SPELLING: &str = "ZBD/planes.zbd";

/// The archive's catalog **key**, which is what the audit compares against: a
/// `ContentId` carries its key, and its own id spelling is the namespace and the
/// key joined.
fn archive_key() -> String {
    cs_content::catalog::baseline::install_file_key(ARCHIVE_SPELLING)
}

/// The catalog id of the authored archive.
fn archive_id() -> ContentId {
    ContentId::from_source(ContentKind::InstallFile, &archive_key())
        .expect("the normalized archive key is a valid id key")
}

/// The node id a root inside the authored archive carries: a `SceneRootRef`
/// requires the root's key to be the container's own key plus the node's stored
/// name, and the production roster discovery builds exactly that pairing.
fn root_id(root: &str) -> cs_content::scene::SceneNodeId {
    cs_content::scene::SceneNodeId::from_content_id(
        ContentId::from_source(ContentKind::SceneNode, &format!("{}.{root}", archive_key()))
            .expect("a container-qualified node key is a valid id key"),
    )
    .expect("the scene node id is valid")
}

/// Designed provenance for a fixture value.
fn designed() -> Provenance {
    Provenance::designed(claim())
}

/// Observed provenance over a byte range of the fixture container.
///
/// The digest is a **designed** stand-in, never a real installation hash: it is
/// there so a span can be built, and the tests below never treat it as evidence
/// that any installation was read. The retail tests build their spans from the
/// real `fingerprint`.
fn observed(offset: u64, length: u64) -> Provenance {
    let digest =
        ContentHash::from_hex("0000000000000000000000000000000000000000000000000000000000000000")
            .expect("sixty-four zeroes are a digest");
    let span = SourceSpan::new(digest, FIXTURE, None, offset, length, None)
        .expect("a span over fixture bytes is valid");
    Provenance::new(claim(), ClaimStatus::ObservedTool, Some(span))
        .expect("an observed_tool claim with a span is valid")
}

/// One authored line: NUL-terminated arguments with their declared count, which
/// is the shape the container's own decoder splits tokens on.
fn line(tokens: &[&[u8]]) -> Vec<u8> {
    let mut data = Vec::new();
    let mut count = 0_u32;
    for token in tokens {
        data.extend_from_slice(token);
        data.push(0);
        count += 1;
    }
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&count.to_le_bytes());
    bytes.extend_from_slice(&data);
    bytes
}

/// Builds one container from `(script name, lines)`.
fn container(scripts: &[(&[u8], Vec<Vec<u8>>)]) -> Vec<u8> {
    use cs_formats::interp::{INDEX_ENTRY_BYTES, INTERP_HEADER_BYTES, NAME_FIELD_BYTES};

    let body_start = (INTERP_HEADER_BYTES + scripts.len() * INDEX_ENTRY_BYTES) as u64;
    let mut body = Vec::new();
    let mut offsets = Vec::new();
    for (_, lines) in scripts {
        offsets.push(body_start + body.len() as u64);
        for data in lines {
            body.extend_from_slice(data);
        }
        body.extend_from_slice(&0_u32.to_le_bytes());
    }
    let mut bytes = Vec::new();
    for word in [0x0897_1119_u32, 7, scripts.len() as u32] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    for ((name, _), offset) in scripts.iter().zip(&offsets) {
        let mut field = [0_u8; NAME_FIELD_BYTES];
        field[..name.len()].copy_from_slice(name);
        bytes.extend_from_slice(&field);
        bytes.extend_from_slice(&1_000_u32.to_le_bytes());
        bytes.extend_from_slice(&(*offset as u32).to_le_bytes());
    }
    bytes.extend_from_slice(&body);
    bytes
}

/// Decodes one authored container through the production decoder.
fn decode(bytes: &[u8]) -> cs_formats::interp::DecodedInterp<'_> {
    use cs_formats::interp::decode_interp;
    use cs_formats::io::ParseContext;

    decode_interp(&mut ParseContext::with_defaults(FIXTURE), bytes)
        .expect("the authored container decodes")
}

/// The camera-command claims every synthetic census test uses.
///
/// Arities are the corpus's own: `CameraSetHorizon horizon`, `NewCamera
/// %camName%` and `CameraSetWindow %winName%` are each a head plus one argument. The two `Consumed` verdicts are the project's own
/// judgement and the three `Unconsumed` verdicts each name what the camera
/// contract would have to gain — a horizon-locked up axis, a viewport binding
/// and a per-window display camera are none of which `cs_app::camera` has.
fn view_claims() -> Vec<ViewCommandClaim> {
    let mut claims = Vec::new();
    for (command, arguments, effect, coverage) in [
        (
            "NewCamera",
            2_usize,
            ViewControlEffect::CreatesCamera,
            ViewControlCoverage::Consumed {
                operation: CameraOperation::AuthoredCamera,
            },
        ),
        (
            "CameraSetActive",
            2,
            ViewControlEffect::ActivatesCamera,
            ViewControlCoverage::Consumed {
                operation: CameraOperation::PlayerRig,
            },
        ),
        (
            "CameraSetWorld",
            2,
            ViewControlEffect::BindsCameraToWorld,
            ViewControlCoverage::Consumed {
                operation: CameraOperation::PlayerRig,
            },
        ),
        (
            "CameraSetWindow",
            2,
            ViewControlEffect::BindsCameraToWindow,
            ViewControlCoverage::Unconsumed {
                reason: "the camera contract has no viewport/window binding".to_owned(),
            },
        ),
        (
            "CameraSetHorizon",
            2,
            ViewControlEffect::LevelsHorizon,
            ViewControlCoverage::Unconsumed {
                reason: "no camera in this contract can level its up axis against a world horizon"
                    .to_owned(),
            },
        ),
        (
            "CameraSetHorizonXZ",
            2,
            ViewControlEffect::LevelsHorizonToZone,
            ViewControlCoverage::Unconsumed {
                reason: "no camera in this contract can level its up axis against a named zone"
                    .to_owned(),
            },
        ),
    ] {
        claims.push(
            ViewCommandClaim::try_new(command, arguments, effect, coverage, observed(0, 8))
                .unwrap_or_else(|error| panic!("the {command} claim is valid, got {error}")),
        );
    }
    claims
}

/// The authored loading-script container every synthetic census test walks.
fn synthetic_interp() -> Vec<u8> {
    container(&[
        (
            b"support\\init.gw",
            vec![
                line(&[b"set", b"worldName", b"world1"]),
                line(&[b"set", b"camName", b"camera1"]),
            ],
        ),
        (
            b"support\\display.gw",
            vec![
                line(&[b"source", b"support\\init.gw"]),
                line(&[b"NewCamera", b"%camName%"]),
                line(&[b"CameraSetActive", b"%camName%"]),
                line(&[b"CameraSetWorld", b"%worldName%"]),
                line(&[b"CameraSetWindow", b"%winName%"]),
            ],
        ),
        (
            b"support\\c1\\load.gw",
            vec![
                line(&[b"FindNode", b"camera1"]),
                line(&[b"CameraSetHorizon", b"horizon"]),
                line(&[b"CameraSetHorizonXZ", b"zone2_cloud_floor"]),
                line(&[b"FindNode", b"spyglass"]),
                line(&[b"CameraSetHorizon", b"horizon"]),
            ],
        ),
    ])
}

/// The authored `support\cockpit.gw` the synthetic binding tests walk: one
/// airframe line, then the node names the original addresses inside it, one node
/// addressed twice so the occurrence count is exercised, and one node addressed
/// after a *different* variable so it is reported rather than attributed.
fn synthetic_cockpit_script() -> Vec<u8> {
    container(&[(
        b"support\\cockpit.gw",
        vec![
            line(&[b"FindNode", b"%player_plane%"]),
            line(&[b"FindSubNode", b"gungauge"]),
            line(&[b"FindSubNode", b"4char_ammo"]),
            line(&[b"FindSubNode", b"gungauge"]),
            line(&[b"FindSubNode", b"missilegauge"]),
            line(&[b"FindNode", b"%other_plane%"]),
            line(&[b"FindSubNode", b"nosedamage"]),
        ],
    )])
}

/// The claim over [`synthetic_cockpit_script`], carrying the spans the walk
/// reports.
fn cockpit_claim() -> CockpitBindingClaim {
    CockpitBindingClaim::try_new(
        "support\\cockpit.gw",
        "FindNode",
        "FindSubNode",
        "%player_plane%",
        observed(12_850, 5_713),
    )
    .expect("the fixture claim is valid")
}

/// One node of an authored airframe archive.
struct Node {
    name: &'static str,
    parent: Option<u32>,
    children: Vec<u32>,
    mesh_index: i32,
}

/// A node record with the stored words the coverage audit reads and every other
/// word at the value the reference asserts, so the array is a valid decoded
/// record rather than a partly-filled one.
fn raw_node(spec: &Node, index: u32) -> cs_formats::gamez::RawNode {
    cs_formats::gamez::RawNode {
        index,
        name: spec.name.to_owned(),
        node_index: 0x0200_0000 | index,
        info: node_info(spec),
        kind: cs_formats::gamez::NodeKind::Object3d(cs_formats::gamez::RawObject3dData {
            flags: 0,
            rotation: [0.0; 3],
            scale: [1.0; 3],
            matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            translation: [0.0; 3],
        }),
        data_offset: 1,
        data_bytes: 148,
        parent: spec.parent,
        children: spec.children.clone(),
    }
}

/// The info words a node stores, with every word the reference asserts at its
/// asserted value except the three the audit reads.
fn node_info(spec: &Node) -> cs_formats::gamez::RawNodeInfo {
    cs_formats::gamez::RawNodeInfo {
        flags: 0x0180_0000,
        zone_id: 255,
        node_type: cs_formats::gamez::NODE_TYPE_OBJECT3D,
        data_ptr: 1,
        mesh_index: spec.mesh_index,
        parent_count: u16::from(spec.parent.is_some()),
        children_count: spec.children.len() as u16,
        unk196: 160,
        unk116: [[0.0; 3]; 2],
        unk140: [[0.0; 3]; 2],
        unk164: [[0.0; 3]; 2],
        area_partition: [-1, -1, 0, 0],
        ..zero_node_info()
    }
}

/// The rest of a node's info words, at the values the reference asserts.
fn zero_node_info() -> cs_formats::gamez::RawNodeInfo {
    cs_formats::gamez::RawNodeInfo {
        flags: 0,
        unk040: 0,
        unk044: 0,
        zone_id: 0,
        node_type: 0,
        data_ptr: 0,
        mesh_index: -1,
        environment_data: 0,
        action_priority: 1,
        action_callback: 0,
        area_partition: [0; 4],
        parent_count: 0,
        children_count: 0,
        parent_array_ptr: 0,
        children_array_ptr: 0,
        unk096: 0,
        unk100: 0,
        unk104: 0,
        unk108: 0,
        unk112: 0,
        unk116: [[0.0; 3]; 2],
        unk140: [[0.0; 3]; 2],
        unk164: [[0.0; 3]; 2],
        unk188: 0,
        unk192: 0,
        unk196: 0,
        unk200: 0,
        unk204: 0,
    }
}

/// The authored airframe archive the synthetic coverage tests audit: two
/// airframes, each with a `gungauge` node that carries a mesh, and one shared
/// node name under the second root that resolves twice.
fn synthetic_archive() -> cs_formats::gamez::GameZNodes {
    let specs = [
        Node {
            name: "player_kestrel",
            parent: None,
            children: vec![1, 2, 3],
            mesh_index: -1,
        },
        Node {
            name: "gungauge",
            parent: Some(0),
            children: Vec::new(),
            mesh_index: 41,
        },
        Node {
            name: "4char_ammo",
            parent: Some(0),
            children: Vec::new(),
            mesh_index: 42,
        },
        Node {
            name: "missilegauge",
            parent: Some(0),
            children: Vec::new(),
            mesh_index: 42,
        },
        Node {
            name: "player_warhawk",
            parent: None,
            children: vec![5, 6],
            mesh_index: -1,
        },
        Node {
            name: "gungauge",
            parent: Some(4),
            children: Vec::new(),
            mesh_index: 43,
        },
        Node {
            name: "4char_ammo",
            parent: Some(4),
            children: Vec::new(),
            mesh_index: -1,
        },
    ];
    let nodes = specs
        .iter()
        .enumerate()
        .map(|(index, spec)| raw_node(spec, index as u32))
        .collect();
    cs_formats::gamez::GameZNodes {
        header: cs_formats::gamez::GameZHeader {
            signature: cs_formats::zbd::GAMEZ_SIGNATURE,
            version: cs_formats::zbd::GAMEZ_VERSION,
            unk08: 0,
            texture_count: 0,
            textures_offset: cs_formats::gamez::GAMEZ_HEADER_BYTES as u32,
            materials_offset: 0,
            meshes_offset: 0,
            node_array_size: specs.len() as u32,
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

/// An airframe the synthetic coverage tests audit.
fn airframe(key: &str, root: &str) -> CockpitAirframe {
    CockpitAirframe::new(
        ContentId::from_source(ContentKind::Airframe, key).expect("a valid fixture airframe id"),
        cs_content::scene::SceneRootRef::new(archive_id(), root_id(root))
            .expect("the root lives in the archive it is bound to"),
    )
}

/// The private evidence directory the captures and the harness share.
fn evidence_dir() -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../private/evidence/F21-D");
    let _ = std::fs::create_dir_all(&path);
    path
}

// ------------------------------------------------- the view-control census ---

/// The census reports what the scripts declare, with the scripts and offsets it
/// measured, and the coverage verdict travels with the row.
#[test]
fn accept_f21_d_the_declared_camera_commands_are_censused_with_their_scripts_and_arguments() {
    let bytes = synthetic_interp();
    let census = discover_view_controls(&decode(&bytes), &view_claims(), FIXTURE, observed(0, 16))
        .expect("every claimed command is written by the authored container");

    assert_eq!(census.container(), FIXTURE);
    assert!(
        census.lines_walked() > 0,
        "the walk read the container it was handed"
    );
    assert_eq!(
        census.occurrences(),
        7,
        "the authored container writes 1 NewCamera, 1 CameraSetActive, 1 CameraSetWorld, \
         1 CameraSetWindow, 2 CameraSetHorizon and 1 CameraSetHorizonXZ"
    );

    let creates = census
        .row("NewCamera")
        .expect("the claimed command has its own row");
    assert_eq!(creates.effect(), ViewControlEffect::CreatesCamera);
    assert_eq!(creates.count(), 1);
    assert_eq!(creates.scripts(), ["support\\display.gw"]);
    let occurrence = &creates.occurrences()[0];
    assert_eq!(
        occurrence.arguments(),
        ["%camName%"],
        "the occurrence keeps the arguments exactly as stored, including the unexpanded variable"
    );
    assert!(
        occurrence.offset() > 0,
        "an occurrence names the byte offset it was read at, not just a count"
    );
    assert_eq!(
        occurrence.provenance(),
        &observed(0, 16),
        "and the install-wide provenance the caller passed unchanged"
    );

    let horizon = census
        .row("CameraSetHorizon")
        .expect("the horizon command has its own row");
    assert_eq!(horizon.count(), 2);
    assert_eq!(
        horizon.scripts(),
        ["support\\c1\\load.gw"],
        "both world-group occurrences come from the one world script that writes it"
    );
    assert_eq!(
        horizon.occurrences()[0].arguments(),
        ["horizon"],
        "the horizon argument is stored verbatim"
    );

    // The offsets are increasing within a command, so a reader can tell the two
    // horizon lines apart and neither is a re-read of the other.
    let offsets: Vec<u64> = horizon
        .occurrences()
        .iter()
        .map(|row| row.offset())
        .collect();
    assert!(
        offsets.windows(2).all(|pair| pair[0] < pair[1]),
        "occurrences are reported in container order: {offsets:?}"
    );

    assert!(
        census.is_clean(),
        "an authored container is walked without findings"
    );
    // The census records every head the container stores, so a claim list that
    // missed a command would be visible rather than silently narrow.
    assert_eq!(
        census.heads().len(),
        9,
        "the authored container stores nine distinct heads: set, source, FindNode and the six \
         claimed camera commands"
    );
    assert_eq!(
        census.heads().get("FindNode"),
        Some(&2),
        "with the count the container really stores for each: camera1 and spyglass"
    );
    let unclaimed: Vec<&str> = census.unclaimed_heads().map(|(head, _)| head).collect();
    assert_eq!(
        unclaimed,
        ["FindNode", "set", "source"],
        "and the heads no claim matched are exactly the non-camera commands"
    );
    assert_eq!(
        census.unconsumed().count(),
        4,
        "the window binding, the horizon levelling and the zone levelling have no consumer, and \
         all four of their occurrences are reported as unconsumed rather than dropped"
    );
    assert_eq!(
        census.unconsumed().count(),
        census
            .row("CameraSetWindow")
            .map(|row| row.count())
            .expect("the window row exists")
            + census
                .row("CameraSetHorizon")
                .map(|row| row.count())
                .expect("the horizon row exists")
            + census
                .row("CameraSetHorizonXZ")
                .map(|row| row.count())
                .expect("the zone row exists"),
        "which is exactly the three unconsumed rows' occurrences and nothing else"
    );
    let window = census
        .row("CameraSetWindow")
        .expect("the window row exists");
    assert_eq!(window.coverage().label(), "unconsumed");
    match window.coverage() {
        ViewControlCoverage::Unconsumed { reason } => assert!(
            reason.contains("window"),
            "an unconsumed verdict says what is missing: {reason}"
        ),
        other => panic!("the window binding is unconsumed, got {other:?}"),
    }
    assert_eq!(
        census
            .row("CameraSetActive")
            .map(|row| row.coverage().label()),
        Some("player_rig"),
        "a consumed verdict names the operation that consumes it"
    );
    assert_eq!(
        census
            .row("CameraSetHorizon")
            .map(|row| row.effect().label()),
        Some("levels_horizon"),
        "a command whose spelling and argument name an operation is classified as one"
    );
    assert_eq!(
        ViewControlEffect::Undetermined.to_string(),
        "undetermined",
        "and a command whose operation nothing readable establishes is reported undetermined \
         rather than filed under an operation nobody measured"
    );
    let undetermined = container(&[(
        b"support\\display.gw",
        vec![line(&[b"CameraSetObjectHSETest", b"off"])],
    )]);
    let undetermined_census = discover_view_controls(
        &decode(&undetermined),
        &[ViewCommandClaim::try_new(
            "CameraSetObjectHSETest",
            2,
            ViewControlEffect::Undetermined,
            ViewControlCoverage::Unconsumed {
                reason: "the hull-sensitivity probe has no counterpart in this contract".to_owned(),
            },
            observed(0, 8),
        )
        .expect("the claim is well formed")],
        FIXTURE,
        observed(0, 16),
    )
    .expect("the container writes the claimed command");
    assert_eq!(
        undetermined_census
            .row("CameraSetObjectHSETest")
            .map(|row| row.effect().label()),
        Some("undetermined"),
        "the census carries the caller's verdict verbatim instead of inferring one"
    );
}

/// A claim the container never writes is refused rather than reported as a
/// confidently empty row, and every malformed claim has its own refusal.
#[test]
fn accept_f21_d_a_claim_the_container_never_writes_is_refused_rather_than_reported_as_no_coverage()
{
    let bytes = synthetic_interp();

    // The command no line of the authored container spells.
    let mut unseen = view_claims();
    unseen.push(
        ViewCommandClaim::try_new(
            "CameraSetRoll",
            2,
            ViewControlEffect::LevelsHorizon,
            ViewControlCoverage::Consumed {
                operation: CameraOperation::PlayerRig,
            },
            observed(0, 8),
        )
        .expect("the claim itself is well formed"),
    );
    assert_eq!(
        discover_view_controls(&decode(&bytes), &unseen, FIXTURE, observed(0, 16)),
        Err(ViewControlError::ClaimUnseen {
            command: "CameraSetRoll".to_owned()
        }),
        "a command the container never writes has no coverage verdict to report"
    );

    // The same spelling claimed twice is a contradiction, not two rows.
    let mut duplicated = view_claims();
    duplicated.push(
        ViewCommandClaim::try_new(
            "NewCamera",
            2,
            ViewControlEffect::CreatesCamera,
            ViewControlCoverage::Consumed {
                operation: CameraOperation::AuthoredCamera,
            },
            observed(0, 8),
        )
        .expect("the claim itself is well formed"),
    );
    assert_eq!(
        discover_view_controls(&decode(&bytes), &duplicated, FIXTURE, observed(0, 16)),
        Err(ViewControlError::DuplicateCommand {
            command: "NewCamera".to_owned()
        }),
        "two claims about one spelling cannot both be census rows"
    );

    assert_eq!(
        discover_view_controls(&decode(&bytes), &[], FIXTURE, observed(0, 16)),
        Err(ViewControlError::EmptyClaim),
        "an empty claim list would be a census of nothing, reported as everything"
    );

    // An `Unconsumed` verdict with no reason is refused: a gap that does not say
    // what is missing cannot be closed.
    assert_eq!(
        ViewCommandClaim::try_new(
            "CameraSetWindow",
            2,
            ViewControlEffect::BindsCameraToWindow,
            ViewControlCoverage::Unconsumed {
                reason: "   ".to_owned()
            },
            observed(0, 8),
        ),
        Err(ViewControlError::UnconsumedWithoutReason {
            command: "CameraSetWindow".to_owned()
        }),
        "an unconsumed verdict must name what the camera contract would have to gain"
    );

    // A command the container stores but only in a shape the claim does not
    // describe is a different failure from one it never stores: saying the
    // corpus lacks it would contradict the container's own bytes.
    let malformed = container(&[(
        b"support\\c1\\load.gw",
        vec![
            line(&[b"CameraSetHorizon"]),
            line(&[b"CameraSetHorizon", b"horizon", b"extra"]),
            line(&[b"CameraSetHorizon", &[0xff, 0xfe, 0x80]]),
        ],
    )]);
    let decoded_malformed = decode(&malformed);
    let mut wrong_arity = view_claims();
    wrong_arity.clear();
    wrong_arity.push(
        ViewCommandClaim::try_new(
            "CameraSetHorizon",
            2,
            ViewControlEffect::LevelsHorizon,
            ViewControlCoverage::Consumed {
                operation: CameraOperation::PlayerRig,
            },
            observed(0, 8),
        )
        .expect("the claim is well formed"),
    );
    let refusal =
        discover_view_controls(&decoded_malformed, &wrong_arity, FIXTURE, observed(0, 16))
            .expect_err("no stored line has the claimed shape, so there is nothing to census");
    let ViewControlError::ClaimUnreadable {
        command,
        stored_lines,
        findings,
    } = &refusal
    else {
        panic!(
            "a command the container stores in another shape is ClaimUnreadable, got {refusal:?}"
        )
    };
    assert_eq!(command, "CameraSetHorizon");
    assert_eq!(
        *stored_lines, 3,
        "and the refusal counts the lines the container really stores with it"
    );
    assert_eq!(
        findings.len(),
        3,
        "carrying every finding that says why none of them was an occurrence, rather than \
         discarding them: {findings:?}"
    );
    assert!(
        refusal.to_string().contains("CameraSetHorizon"),
        "the message names the command: {refusal}"
    );

    // A bounded name: an unbounded string from an unfingerprinted file cannot be
    // echoed into a report or a key.
    let long = "x".repeat(cs_app::camera::coverage::MAX_DECLARED_NAME + 1);
    assert_eq!(
        ViewCommandClaim::try_new(
            long,
            2,
            ViewControlEffect::CreatesCamera,
            ViewControlCoverage::Consumed {
                operation: CameraOperation::AuthoredCamera
            },
            observed(0, 8),
        ),
        Err(ViewControlError::DeclaredName {
            field: "command",
            len: cs_app::camera::coverage::MAX_DECLARED_NAME + 1
        }),
        "a claimed spelling longer than the bound is refused"
    );
}

/// A line stored with another shape is a finding on the row it belongs to, not a
/// silent drop and not a repaired line.
#[test]
fn accept_f21_d_a_line_the_claim_does_not_describe_is_reported_instead_of_dropped_or_repaired() {
    // One `CameraSetHorizon` line with no argument, one with three, and one whose
    // argument is not ASCII text — all three written by the container.
    let bytes = container(&[(
        b"support\\c1\\load.gw",
        vec![
            line(&[b"CameraSetHorizon"]),
            line(&[b"CameraSetHorizon", b"horizon", b"extra"]),
            line(&[b"CameraSetHorizon", &[0xff, 0xfe, 0x80]]),
            line(&[b"CameraSetHorizon", b"horizon"]),
        ],
    )]);
    // Only the one command this container writes is claimed: a claim the
    // container does not back is refused outright (see the census test), so a
    // claim list has to describe this container to reach the findings at all.
    let claim = |command: &str| {
        ViewCommandClaim::try_new(
            command,
            2,
            ViewControlEffect::LevelsHorizon,
            ViewControlCoverage::Consumed {
                operation: CameraOperation::PlayerRig,
            },
            observed(0, 8),
        )
        .expect("the claim is well formed")
    };
    let census = discover_view_controls(
        &decode(&bytes),
        &[claim("CameraSetHorizon")],
        FIXTURE,
        observed(0, 16),
    )
    .expect("the one well-formed line is enough for the claim to have a row");

    let row = census
        .row("CameraSetHorizon")
        .expect("the claimed command has its row");
    assert_eq!(
        row.count(),
        1,
        "only the one line the claim describes is an occurrence"
    );
    assert!(
        !census.is_clean(),
        "a container that also stores shapes the claim does not describe is not clean"
    );
    let findings = row.findings();
    assert_eq!(
        findings.len(),
        3,
        "all three unreadable lines are reported, none merged and none dropped: {findings:?}"
    );
    assert!(
        findings.iter().all(|finding| matches!(
            finding,
            ViewControlFinding::LineUnreadable { .. } | ViewControlFinding::ArgumentNotAscii { .. }
        )),
        "and each is one of the two named shapes: {findings:?}"
    );
    let arities: Vec<usize> = findings
        .iter()
        .filter_map(|finding| match finding {
            ViewControlFinding::LineUnreadable {
                stored_arguments, ..
            } => Some(*stored_arguments),
            _ => None,
        })
        .collect();
    assert_eq!(
        arities,
        [1, 3],
        "the reported arity is what the container stores, not the claimed one"
    );
    assert!(
        findings
            .iter()
            .any(|finding| matches!(finding, ViewControlFinding::ArgumentNotAscii { .. })),
        "a non-ASCII argument is refused rather than lossily decoded into a name"
    );
}

// -------------------------------------------------- the cockpit bindings ---

/// The binding walk reports the declared nodes in stored order, counts the
/// repeats, and refuses to attribute a node to an aircraft nothing named.
#[test]
fn accept_f21_d_the_cockpit_bindings_the_original_declares_are_measured_in_stored_order() {
    let bytes = synthetic_cockpit_script();
    let discovery =
        discover_cockpit_bindings(&decode(&bytes), &cockpit_claim(), FIXTURE, observed(0, 16))
            .expect("the claimed script is written by the authored container");

    assert_eq!(discovery.script(), "support\\cockpit.gw");
    assert_eq!(
        discovery.container(),
        FIXTURE,
        "the container label the caller passed is the one reported"
    );
    let nodes: Vec<&str> = discovery
        .bindings()
        .iter()
        .map(|binding| binding.node())
        .collect();
    assert_eq!(
        nodes,
        ["gungauge", "4char_ammo", "missilegauge"],
        "the bindings are the declared set in first-use order, deduplicated by name"
    );
    let gauge = discovery.binding("gungauge").expect("the binding exists");
    assert_eq!(
        gauge.occurrences(),
        2,
        "a node addressed by two lines is one binding with two occurrences"
    );
    assert_eq!(
        discovery
            .binding("4char_ammo")
            .map(|binding| binding.occurrences()),
        Some(1)
    );
    assert!(
        discovery.binding("nosedamage").is_none(),
        "a node addressed after a variable the claim did not declare is not a binding of this \
         aircraft"
    );
    // Lookups match the way the walk matches names, on stored bytes and without
    // regard to case: a lookup that missed the row the walk itself produced
    // would report a binding that the container really declares as absent.
    assert!(
        discovery.binding("GUNGAUGE").is_some(),
        "a name the container stores in another case is the same binding"
    );
    assert!(
        discovery.binding("GunGauge").is_some(),
        "and the same whatever the case of the caller"
    );
    assert!(
        !discovery.is_empty() && discovery.lines_walked() > 0,
        "the discovery says how much it read"
    );
    assert!(
        !discovery.is_clean(),
        "the foreign variable is a finding, so a reader is not left thinking the walk was silent"
    );
    assert!(
        discovery
            .findings()
            .iter()
            .any(|finding| matches!(finding, CockpitFinding::ForeignAirframeVariable { .. })),
        "and it is the named finding: {:?}",
        discovery.findings()
    );
    // The two failure shapes stay distinct. This container names a foreign
    // variable, and the subnode line after it belongs to *that* aircraft rather
    // than to one this claim is about — so both facts are reported, each by its
    // own name, and neither is folded into the other's count.
    let orphans: Vec<&CockpitFinding> = discovery
        .findings()
        .iter()
        .filter(|finding| matches!(finding, CockpitFinding::SubnodeOutsideAirframe { .. }))
        .collect();
    assert_eq!(
        orphans.len(),
        1,
        "the subnode line that follows the foreign variable is reported as unattributable"
    );
    match orphans[0] {
        CockpitFinding::SubnodeOutsideAirframe { node, .. } => assert_eq!(
            node, "nosedamage",
            "and it names the node nothing above it claimed"
        ),
        other => panic!("an unattributable subnode line is SubnodeOutsideAirframe, got {other:?}"),
    }
    assert_eq!(
        discovery
            .binding("gungauge")
            .map(|binding| binding.first_at()),
        discovery
            .binding("gungauge")
            .map(|binding| binding.first_at()),
        "the first offset is a property of the binding, so a reader can find the line"
    );
    let first = discovery
        .binding("gungauge")
        .expect("the binding exists")
        .first_at();
    let second = discovery
        .binding("4char_ammo")
        .expect("the binding exists")
        .first_at();
    assert!(
        first < second,
        "and it increases along the script: {first} then {second}"
    );
}

/// A claim about a script the container does not hold, or holds twice, or holds
/// without bindings, is refused by name.
#[test]
fn accept_f21_d_a_cockpit_claim_the_container_does_not_back_is_refused() {
    let bytes = synthetic_cockpit_script();
    let decoded = decode(&bytes);

    let absent = CockpitBindingClaim::try_new(
        "support\\no_such.gw",
        "FindNode",
        "FindSubNode",
        "%player_plane%",
        observed(0, 8),
    )
    .expect("the claim is well formed");
    assert_eq!(
        discover_cockpit_bindings(&decoded, &absent, FIXTURE, observed(0, 16)),
        Err(CockpitCoverageError::ScriptAbsent {
            script: "support\\no_such.gw".to_owned()
        }),
        "a container that holds no such script declares no bindings"
    );

    // Two scripts with the same name: which one declares the bindings is not
    // answerable, so the walk refuses instead of picking the first.
    let duplicated = container(&[
        (
            b"support\\cockpit.gw",
            vec![
                line(&[b"FindNode", b"%player_plane%"]),
                line(&[b"FindSubNode", b"gungauge"]),
            ],
        ),
        (
            b"support\\cockpit.gw",
            vec![
                line(&[b"FindNode", b"%player_plane%"]),
                line(&[b"FindSubNode", b"gungauge"]),
            ],
        ),
    ]);
    assert_eq!(
        discover_cockpit_bindings(
            &decode(&duplicated),
            &cockpit_claim(),
            FIXTURE,
            observed(0, 16)
        ),
        Err(CockpitCoverageError::ScriptAmbiguous {
            script: "support\\cockpit.gw".to_owned(),
            matches: 2
        }),
        "an ambiguous script name is refused, not resolved by position"
    );

    // A script that declares no binding at all.
    let empty = container(&[(
        b"support\\cockpit.gw",
        vec![line(&[b"source", b"support\\init.gw"])],
    )]);
    assert_eq!(
        discover_cockpit_bindings(&decode(&empty), &cockpit_claim(), FIXTURE, observed(0, 16)),
        Err(CockpitCoverageError::NoBindings {
            script: "support\\cockpit.gw".to_owned()
        }),
        "a discovery that found nothing has not discovered anything"
    );

    // The claim's own validation: one spelling for both commands would make
    // every line match both.
    assert_eq!(
        CockpitBindingClaim::try_new(
            "support\\cockpit.gw",
            "FindNode",
            "findnode",
            "%player_plane%",
            observed(0, 8),
        ),
        Err(CockpitCoverageError::RepeatedCommand {
            command: "FindNode".to_owned()
        }),
        "the two commands cannot be the same spelling"
    );
    assert_eq!(
        CockpitBindingClaim::try_new(
            "",
            "FindNode",
            "FindSubNode",
            "%player_plane%",
            observed(0, 8),
        ),
        Err(CockpitCoverageError::DeclaredName {
            field: "script",
            len: 0
        }),
        "an empty script name is refused"
    );
}

/// Each binding is checked inside its **own** airframe's subtree, and every way
/// of not resolving is a named state rather than an absent row.
#[test]
fn accept_f21_d_cockpit_coverage_resolves_each_binding_against_its_own_airframe_subtree() {
    let bytes = synthetic_cockpit_script();
    let discovery =
        discover_cockpit_bindings(&decode(&bytes), &cockpit_claim(), FIXTURE, observed(0, 16))
            .expect("the authored container declares bindings");
    let archive = synthetic_archive();
    let airframes = [
        airframe("synthetic.kestrel", "player_kestrel"),
        airframe("synthetic.warhawk", "player_warhawk"),
    ];
    let report = audit_cockpit_coverage(&discovery, &airframes, &archive, &archive_id())
        .expect("both roots exist");

    assert_eq!(report.declared_per_airframe(), discovery.bindings().len());
    assert_eq!(report.rows().len(), 2);

    let kestrel = report
        .row("airframe/synthetic.kestrel")
        .expect("the first airframe has a row");
    assert_eq!(
        kestrel.root(),
        "player_kestrel",
        "the stored node name the archive lookup used"
    );
    assert_eq!(
        kestrel.root_ref(),
        format!("{}.player_kestrel", archive_key()),
        "and the container-qualified key a catalog consumer addresses it by"
    );
    assert_eq!(
        kestrel.bound(),
        3,
        "every declared node exists under the kestrel root with a mesh"
    );
    assert!(kestrel.is_complete());
    assert_eq!(
        kestrel.unresolved().len(),
        0,
        "a complete airframe reports nothing unresolved"
    );
    assert_eq!(
        kestrel.drawable_meshes(),
        [
            ("gungauge", 41_u32),
            ("4char_ammo", 42),
            ("missilegauge", 42),
        ],
        "each binding resolves to the mesh index its own node stores"
    );
    assert_eq!(
        kestrel.verified_bindings(),
        vec![
            CockpitBindingSource::ModelNode {
                node: "gungauge".to_owned()
            },
            CockpitBindingSource::ModelNode {
                node: "4char_ammo".to_owned()
            },
            CockpitBindingSource::ModelNode {
                node: "missilegauge".to_owned()
            },
        ],
        "the verified bindings are the names an importer may bind a viewpoint to, and nothing \
         else: F21 non-negotiable behavior 1 is answered by this list"
    );

    let warhawk = report
        .row("airframe/synthetic.warhawk")
        .expect("the second airframe has a row");
    assert_eq!(
        warhawk.bound(),
        1,
        "only `gungauge` resolves under the warhawk root"
    );
    assert!(!warhawk.is_complete());
    assert_eq!(
        warhawk
            .unresolved()
            .iter()
            .map(|binding| binding.node())
            .collect::<Vec<_>>(),
        ["4char_ammo", "missilegauge"],
        "an unresolved binding is reported by name, and a node with no mesh is one of them"
    );
    assert_eq!(
        warhawk.drawable_meshes(),
        [("gungauge", 43)],
        "and the drawable geometry is the warhawk's own mesh, never the kestrel's 41"
    );
    assert_eq!(
        warhawk.verified_bindings(),
        vec![CockpitBindingSource::ModelNode {
            node: "gungauge".to_owned()
        }],
        "only a resolved binding is a *verified* one: a name the original addresses but this \
         airframe does not carry must not become a viewpoint binding, which is exactly the \
         substitution F21 non-negotiable behavior 1 forbids"
    );
    assert!(
        kestrel.verified_bindings() != warhawk.verified_bindings(),
        "and the two airframes' verified bindings are their own, not one shared list"
    );
    assert!(
        !report.is_complete(),
        "the report is complete only when every airframe resolved every binding"
    );
    // The same rule on stored bytes: the root key and the archive's node name
    // come from two containers whose encodings this project has not
    // established, so a lookup that demanded an exact spelling would report an
    // aircraft as missing from an archive that holds it.
    assert!(
        report.row("AIRFRAME/SYNTHETIC.KESTREL").is_some(),
        "a row the audit wrote is found whatever the case of the caller's id"
    );
    assert_eq!(
        report.drawable_meshes(),
        [
            ("airframe/synthetic.kestrel", "gungauge", 41),
            ("airframe/synthetic.kestrel", "4char_ammo", 42),
            ("airframe/synthetic.kestrel", "missilegauge", 42),
            ("airframe/synthetic.warhawk", "gungauge", 43),
        ],
        "the report's drawable list is every airframe's own geometry, in row order"
    );

    // The node that exists but references no mesh is a named state: present,
    // named by the original, and not drawable.
    let ammo = warhawk
        .bindings()
        .iter()
        .find(|binding| binding.node() == "4char_ammo")
        .expect("the binding has a row");
    assert_eq!(
        ammo.coverage(),
        &CockpitNodeCoverage::NoMesh { node_index: 6 },
        "a node with no mesh association is not a cockpit geometry binding"
    );
    assert_eq!(ammo.coverage().mesh_index(), None);
    assert_eq!(ammo.coverage().node_index(), Some(6));

    // An airframe the archive does not hold is refused, not reported empty.
    assert_eq!(
        audit_cockpit_coverage(
            &discovery,
            &[airframe("synthetic.ghost", "player_ghost")],
            &archive,
            &archive_id(),
        ),
        Err(CockpitCoverageError::RootMissing {
            airframe: "airframe/synthetic.ghost".to_owned(),
            root: "player_ghost".to_owned()
        }),
        "\"no cockpit\" and \"no aircraft\" are different findings"
    );

    // Two airframes over one root would attribute one aircraft's geometry to
    // another.
    assert_eq!(
        audit_cockpit_coverage(
            &discovery,
            &[
                airframe("synthetic.kestrel", "player_kestrel"),
                airframe("synthetic.clone", "player_kestrel")
            ],
            &archive,
            &archive_id(),
        ),
        Err(CockpitCoverageError::DuplicateRoot {
            root: "player_kestrel".to_owned()
        }),
        "a duplicated root is refused before any row is written"
    );

    // The archive's stored name is matched the way the script's name is. Here the
    // container stores the root and its gauges in another case than the script
    // spells them, which an exact comparison would report as an archive that
    // holds no such aircraft at all.
    let recased = [
        Node {
            name: "PLAYER_KESTREL",
            parent: None,
            children: vec![1, 2, 3],
            mesh_index: -1,
        },
        Node {
            name: "GUNGAUGE",
            parent: Some(0),
            children: Vec::new(),
            mesh_index: 41,
        },
        Node {
            name: "4char_ammo",
            parent: Some(0),
            children: Vec::new(),
            mesh_index: 42,
        },
        Node {
            name: "MISSILEGAUGE",
            parent: Some(0),
            children: Vec::new(),
            mesh_index: 42,
        },
    ];
    let recased_archive = cs_formats::gamez::GameZNodes {
        nodes: recased
            .iter()
            .enumerate()
            .map(|(index, spec)| raw_node(spec, index as u32))
            .collect(),
        ..synthetic_archive()
    };
    let recased_report = audit_cockpit_coverage(
        &discovery,
        &[airframe("synthetic.kestrel", "player_kestrel")],
        &recased_archive,
        &archive_id(),
    )
    .expect("the root is in the archive whatever case it is stored in");
    let recased_row = recased_report
        .row("airframe/synthetic.kestrel")
        .expect("the airframe has a row");
    assert_eq!(
        recased_row.drawable_meshes(),
        [("gungauge", 41), ("4char_ammo", 42), ("missilegauge", 42)],
        "and every binding resolves to the recased node that carries its mesh, rather than \
         reporting an aircraft the archive holds as missing"
    );
    assert_eq!(
        recased_row.root(),
        "PLAYER_KESTREL",
        "the row reports the name the archive stores, not the spelling the caller passed"
    );

    // An airframe that lives in another archive is not audited against this one.
    let elsewhere = CockpitAirframe::new(
        ContentId::from_source(ContentKind::Airframe, "synthetic.elsewhere").expect("a valid id"),
        cs_content::scene::SceneRootRef::new(
            ContentId::from_source(
                ContentKind::InstallFile,
                &cs_content::catalog::baseline::install_file_key("ZBD/c1/gamez.zbd"),
            )
            .expect("the normalized key is a valid id key"),
            cs_content::scene::SceneNodeId::from_content_id(
                ContentId::from_source(
                    ContentKind::SceneNode,
                    &format!(
                        "{}.player_kestrel",
                        cs_content::catalog::baseline::install_file_key("ZBD/c1/gamez.zbd")
                    ),
                )
                .expect("a container-qualified node key is a valid id key"),
            )
            .expect("the scene node id is valid"),
        )
        .expect("the root lives in the archive it is bound to"),
    );
    assert_eq!(
        audit_cockpit_coverage(&discovery, &[elsewhere], &archive, &archive_id()),
        Err(CockpitCoverageError::ContainerMismatch {
            airframe: "airframe/synthetic.elsewhere".to_owned(),
            declared: format!(
                "install_file/{}",
                cs_content::catalog::baseline::install_file_key("ZBD/c1/gamez.zbd")
            ),
            measured: archive_id().as_str().to_owned()
        }),
        "an airframe is audited against the archive it declares, never one that happens to open"
    );
}

/// Two nodes of one aircraft carry the same name: the coverage row says so
/// rather than picking one and calling it the binding.
#[test]
fn accept_f21_d_a_node_name_an_airframe_reuses_is_reported_ambiguous_not_resolved_by_position() {
    let bytes = synthetic_cockpit_script();
    let discovery =
        discover_cockpit_bindings(&decode(&bytes), &cockpit_claim(), FIXTURE, observed(0, 16))
            .expect("the authored container declares bindings");
    let specs = [
        Node {
            name: "player_kestrel",
            parent: None,
            children: vec![1, 2, 3, 4],
            mesh_index: -1,
        },
        Node {
            name: "gungauge",
            parent: Some(0),
            children: Vec::new(),
            mesh_index: 41,
        },
        Node {
            name: "gungauge",
            parent: Some(0),
            children: Vec::new(),
            mesh_index: 44,
        },
        Node {
            name: "4char_ammo",
            parent: Some(0),
            children: Vec::new(),
            mesh_index: 42,
        },
        Node {
            name: "missilegauge",
            parent: Some(0),
            children: Vec::new(),
            mesh_index: 42,
        },
    ];
    let archive = cs_formats::gamez::GameZNodes {
        nodes: specs
            .iter()
            .enumerate()
            .map(|(index, spec)| raw_node(spec, index as u32))
            .collect(),
        ..synthetic_archive()
    };
    let report = audit_cockpit_coverage(
        &discovery,
        &[airframe("synthetic.kestrel", "player_kestrel")],
        &archive,
        &archive_id(),
    )
    .expect("the root exists");

    let row = report
        .row("airframe/synthetic.kestrel")
        .expect("the airframe has a row");
    assert_eq!(
        row.bindings()[0].coverage(),
        &CockpitNodeCoverage::Ambiguous {
            node_indices: vec![1, 2]
        },
        "two nodes of one subtree share the name, so no single geometry is named for it"
    );
    assert_eq!(
        row.drawable_meshes(),
        [("4char_ammo", 42), ("missilegauge", 42),],
        "an ambiguous binding contributes no drawable geometry, and the other two still do"
    );
}

/// The subtree walk is the root's children closure and never leaves it, even
/// when the root's own node names a parent: a world node above every airframe
/// must not drag the *other* airframes' nodes into one aircraft's coverage row.
#[test]
fn accept_f21_d_a_subtree_walk_never_leaves_the_aircraft_it_started_from() {
    let bytes = synthetic_cockpit_script();
    let discovery =
        discover_cockpit_bindings(&decode(&bytes), &cockpit_claim(), FIXTURE, observed(0, 16))
            .expect("the authored container declares bindings");
    // A world node owns both airframe roots and one unrelated child, and the
    // kestrel root names the world as its parent — a shape the corpus's parent
    // and child words can both produce.
    let specs = [
        Node {
            name: "world1",
            parent: None,
            children: vec![1, 2, 3],
            mesh_index: -1,
        },
        Node {
            name: "player_kestrel",
            parent: Some(0),
            children: vec![4, 5],
            mesh_index: -1,
        },
        Node {
            name: "player_warhawk",
            parent: Some(0),
            children: vec![6],
            mesh_index: -1,
        },
        Node {
            name: "unrelated",
            parent: Some(0),
            children: Vec::new(),
            mesh_index: 9,
        },
        Node {
            name: "gungauge",
            parent: Some(1),
            children: Vec::new(),
            mesh_index: 41,
        },
        Node {
            name: "missilegauge",
            parent: Some(1),
            children: Vec::new(),
            mesh_index: 42,
        },
        Node {
            name: "gungauge",
            parent: Some(2),
            children: Vec::new(),
            mesh_index: 43,
        },
    ];
    let archive = cs_formats::gamez::GameZNodes {
        nodes: specs
            .iter()
            .enumerate()
            .map(|(index, spec)| raw_node(spec, index as u32))
            .collect(),
        ..synthetic_archive()
    };
    let report = audit_cockpit_coverage(
        &discovery,
        &[airframe("synthetic.kestrel", "player_kestrel")],
        &archive,
        &archive_id(),
    )
    .expect("the root exists");
    let row = report
        .row("airframe/synthetic.kestrel")
        .expect("the airframe has a row");

    assert_eq!(
        row.subtree_nodes(),
        3,
        "the root and its two children, and nothing else: not the world node above it, not the \
     warhawk under it, not the world's unrelated child"
    );
    assert_eq!(
        row.drawable_meshes(),
        [("gungauge", 41), ("missilegauge", 42)],
        "and the geometry resolved is the kestrel's own, never the warhawk's 43"
    );
    assert!(
        row.bindings().iter().all(|binding| {
            binding
                .coverage()
                .node_index()
                .is_none_or(|index| index == 4 || index == 5)
        }),
        "every resolved node index lies inside the kestrel's own subtree: {:?}",
        row.bindings()
    );
}

/// However complete the bindings are, the pilot's eye stays undeclared: no
/// verified binding means a cockpit mode may be declared, not that a viewpoint
/// may be invented.
#[test]
fn accept_f21_d_the_eye_placement_stays_undeclared_however_complete_the_bindings_are() {
    assert_eq!(
        eye_placement(),
        CockpitEyeCoverage::Undeclared,
        "no file this audit reads declares where the pilot's eye sits, so the audit reports it \
         undeclared instead of accepting a value"
    );
    assert_eq!(eye_placement().label(), "undeclared");

    // The strongest coverage the audit can produce, and the eye is still
    // undeclared: a verified binding is a *name*, and a viewpoint needs a
    // transform the original does not put in a readable file.
    let bytes = synthetic_cockpit_script();
    let discovery =
        discover_cockpit_bindings(&decode(&bytes), &cockpit_claim(), FIXTURE, observed(0, 16))
            .expect("the authored container declares bindings");
    let report = audit_cockpit_coverage(
        &discovery,
        &[airframe("synthetic.kestrel", "player_kestrel")],
        &synthetic_archive(),
        &archive_id(),
    )
    .expect("the root exists");
    let row = report
        .row("airframe/synthetic.kestrel")
        .expect("the airframe has a row");
    assert_eq!(row.bound(), 3, "the audit resolved every declared binding");
    assert_eq!(report.eye(), CockpitEyeCoverage::Undeclared);

    // And the engine agrees: a cockpit mode whose viewpoint orientation is an
    // explicit unknown still refuses to lower, which is what keeps the audit's
    // `Undeclared` from being a value someone can fill in downstream.
    use cs_app::camera::lower_camera_modes;
    use cs_content::cameras::{
        AspectFraming, AspectRatio, BodyOffset, CameraModeKind, CockpitBindingSource,
        CockpitViewpoint, DeclaredCameraMode, DeclaredCameraModes, DeclaredPlacement, FovAxis,
        Magnification, ProjectionPolicy,
    };
    use cs_types::content::{Known, Origin, Resolved};
    use cs_types::space::{Meters, Radians};

    let undeclared = CockpitViewpoint::try_new(
        CockpitBindingSource::ModelNode {
            // A *measured* name from the fixture's own verified bindings, which
            // is what makes this the interesting case: a verified name does not
            // make the eye known.
            node: row.verified_bindings()[0].name().to_owned(),
        },
        BodyOffset::new(Meters(0.0), Meters(1.2), Meters(1.5)).expect("a finite offset"),
        Resolved::Unknown {
            claim_id: claim(),
            reason: "f21d: the original declares no eye transform in any readable file".to_owned(),
        },
        Resolved::Known(Known::new(Radians(0.0), designed())),
    )
    .expect("a viewpoint with an unknown orientation is a valid declared state");
    let mode = DeclaredCameraMode::try_new(
        CameraModeKind::Cockpit,
        ProjectionPolicy {
            fov: Resolved::Known(Known::new(Radians(std::f64::consts::FRAC_PI_3), designed())),
            fov_axis: Resolved::Known(Known::new(FovAxis::Vertical, designed())),
            reference_aspect: Resolved::Known(Known::new(AspectRatio::FOUR_THREE, designed())),
            framing: Resolved::Known(Known::new(AspectFraming::PreserveVertical, designed())),
            near_m: Resolved::Known(Known::new(Meters(0.1), designed())),
            far_m: Resolved::Known(Known::new(Meters(10_000.0), designed())),
        },
        Resolved::Known(Known::new(Magnification::ONE, designed())),
        Resolved::Known(Known::new(false, designed())),
        DeclaredPlacement::at_cockpit(undeclared),
        Resolved::Known(Known::new(
            cs_content::cameras::LookLimits::new(Radians(1.0), Radians(0.5))
                .expect("the fixture limits are in range"),
            designed(),
        )),
    )
    .expect("a cockpit mode with a verified binding name is a valid declaration");
    let modes = DeclaredCameraModes::try_new(
        ContentId::from_source(ContentKind::Airframe, "synthetic.kestrel").expect("a valid id"),
        Origin::SyntheticFixture,
        CameraModeKind::Cockpit,
        vec![mode],
        designed(),
    )
    .expect("one mode and a matching default is a valid set");
    let refused = lower_camera_modes(&modes).expect_err("an undeclared eye refuses to lower");
    assert!(
        refused.to_string().contains("yaw"),
        "the refusal names the unknown field, so a reader can see what is missing: {refused}"
    );
}

// -------------------------------------------------------------- the GPU ----

/// The capture half of the stage: a **bound** cockpit binding's geometry draws
/// on a real adapter, through the production upload and the production capture
/// path.
///
/// The mesh here is authored fixture geometry, and the reason is stated rather
/// than hidden: this test is evidence that the *capture* draws what a coverage
/// row vouches for and that a refusal deletes the file it would have written. The
/// mesh the retail corpus binds for a real cockpit node is drawn by
/// `accept_f21_d_retail_a_bound_cockpit_mesh_draws_a_measured_frame_on_the_gpu`,
/// which needs both a GPU and `CS_GAME_DIR`.
#[test]
#[ignore = "requires a GPU: CI selects no adapter, so run it with --include-ignored"]
fn accept_f21_d_a_capture_of_a_bound_binding_mesh_draws_and_a_refusal_writes_no_file() {
    use cs_app::world::gpu_capture::{CaptureRequest, GpuCaptureError, capture_world_mesh};

    let render = fixture_gauge();
    let png = evidence_dir().join("f21-d-fixture-cockpit-binding.png");
    let capture = capture_world_mesh(&CaptureRequest {
        group: "f21-d-fixture",
        mesh_index: 41,
        render: &render,
        unknowns: &[],
        png: &png,
    })
    .expect("a real adapter draws the bound binding's mesh and returns a measured capture");

    assert!(
        capture.distinct_luminance > 1 && capture.covered_pixels > 0,
        "a frame with one luminance level is the background alone: {capture:?}"
    );
    assert!(capture.drew_geometry());
    assert!(
        !capture.adapter.contains("no adapter reported"),
        "the capture names the adapter the driver selected, got {:?}",
        capture.adapter
    );
    assert_eq!(
        capture.mesh_index, 41,
        "the capture is of the mesh the coverage row named"
    );
    assert_eq!(
        capture.triangles, 8,
        "four authored quads, two triangles each"
    );
    let bytes = std::fs::read(&png).expect("the PNG is on disk");
    assert_eq!(cs_assets::install::sha256(&bytes), capture.png_sha256);
    assert_eq!(bytes.len() as u64, capture.png_bytes);

    // The negative half, on the same adapter: a mesh with nothing in it is
    // refused by name and leaves no file that could be read as evidence.
    let refused_png = evidence_dir().join("f21-d-fixture-empty.png");
    let _ = std::fs::remove_file(&refused_png);
    let empty = RenderMesh::build(&RawMesh {
        positions: vec![[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]],
        normals: Vec::new(),
        polygons: Vec::new(),
    })
    .expect("a mesh with no polygon has nothing to validate");
    assert!(
        matches!(
            capture_world_mesh(&CaptureRequest {
                group: "f21-d-fixture",
                mesh_index: 41,
                render: &empty,
                unknowns: &[],
                png: &refused_png,
            }),
            Err(GpuCaptureError::EmptyMesh { groups: 0, .. })
        ),
        "an empty mesh is refused before an adapter is even asked for"
    );
    assert!(
        !refused_png.exists(),
        "a refused capture leaves no file that reads like evidence"
    );
}

/// Authored fixture geometry in the shape a bound cockpit binding resolves to: a
/// flat instrument quad with a raised bezel, built through the one production
/// constructor so the capture draws exactly what a real binding would.
fn fixture_gauge() -> RenderMesh {
    let positions: Vec<[f32; 3]> = vec![
        [-0.5, 0.0, -0.5],
        [0.5, 0.0, -0.5],
        [0.5, 0.0, 0.5],
        [-0.5, 0.0, 0.5],
        [-0.5, 0.5, -0.5],
        [0.5, 0.5, -0.5],
        [0.5, 0.5, 0.5],
        [-0.5, 0.5, 0.5],
    ];
    let quads: [[u32; 4]; 4] = [[0, 1, 2, 3], [4, 5, 6, 7], [0, 1, 5, 4], [2, 3, 7, 6]];
    let polygons = quads
        .iter()
        .map(|quad| cs_formats::gamez::RawPolygon {
            kind: PrimitiveKind::Polygon,
            raw_flags: 0,
            material: 0,
            corners: quad
                .iter()
                .map(|index| RawCorner {
                    position: *index,
                    normal: None,
                    uv: Some([0.0, 0.0]),
                    color: None,
                })
                .collect(),
        })
        .collect();
    RenderMesh::build(&RawMesh {
        positions,
        normals: Vec::new(),
        polygons,
    })
    .expect("the authored gauge geometry has a decodable outline")
}

// ------------------------------------------------------------- the retail --

/// The census over the **real** loading-script container, with the real
/// fingerprints, so the numbers the finding quotes are measured here rather than
/// transcribed.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f21_d_retail_the_installation_declares_its_camera_commands_and_cockpit_bindings() {
    use cs_formats::interp::decode_interp;
    use cs_formats::io::ParseContext;

    let game_dir = retail_dir();
    let discovery = cs_assets::install::discover(&game_dir).expect("production discovery reads it");
    let install_sha256 = cs_assets::install::fingerprint(&discovery.manifest);
    let container_label = "ZBD/interp.zbd";
    let bytes = std::fs::read(game_dir.join("ZBD").join("interp.zbd"))
        .expect("the loading-script container must be there");
    // The bytes this test walks are the bytes the installation's own manifest
    // declares: production discovery hashed every inventoried file, so a
    // mismatch here would mean the census describes a different installation
    // than the one the evidence record fingerprints.
    let declared = discovery
        .manifest
        .files
        .iter()
        .find(|row| row.relative_spelling.logical_key() == "zbd/interp.zbd")
        .map(|row| row.sha256)
        .expect("the loading-script container is inventoried");
    assert_eq!(
        cs_assets::install::sha256(&bytes),
        declared,
        "the bytes the census walks are the bytes the installation manifest \
         declares for zbd/interp.zbd"
    );
    let decoded = decode_interp(&mut ParseContext::with_defaults(container_label), &bytes)
        .expect("the real container decodes");

    // --- the cockpit binding set, as the original's own script declares it ---
    let claim = retail_cockpit_claim(install_sha256);
    let bindings = discover_cockpit_bindings(
        &decoded,
        &claim,
        container_label,
        retail_provenance(install_sha256, container_label, 12_850, 5_713),
    )
    .expect("the claimed script is the one the container holds");

    assert_eq!(bindings.script(), "support\\cockpit.gw");
    let nodes: Vec<&str> = bindings
        .bindings()
        .iter()
        .map(|binding| binding.node())
        .collect();
    assert_eq!(
        nodes, RETAIL_COCKPIT_BINDINGS,
        "the binding set is measured from the real script: {nodes:?}"
    );
    assert!(
        bindings.lines_walked() > 100,
        "the walk read the whole declared script, not a prefix: {}",
        bindings.lines_walked()
    );
    for binding in bindings.bindings() {
        assert!(
            binding.first_at() >= 12_850 && binding.first_at() < 18_563,
            "every binding's offset falls inside the script's own extent: {} at {}",
            binding.node(),
            binding.first_at()
        );
    }
    // The claim describes the real script completely: every line of it is a
    // `FindNode %player_plane%` or a `FindSubNode <name>` of arity two, so the
    // walk has nothing to report. That is a measured result about the corpus and
    // not an assumption — a line outside the claim's two shapes would appear here
    // as a named finding rather than be skipped.
    assert!(
        bindings.is_clean(),
        "the real script writes only the two shapes the claim declares: {:?}",
        bindings.findings()
    );
    assert!(
        bindings
            .bindings()
            .iter()
            .all(|binding| binding.occurrences() >= 1),
        "and every binding is addressed at least once"
    );
    eprintln!(
        "F21-D retail: {} distinct cockpit bindings over {} lines of support\\cockpit.gw",
        bindings.bindings().len(),
        bindings.lines_walked()
    );

    // --- the camera-command census over the same container ---
    let census = discover_view_controls(
        &decoded,
        &retail_view_claims(install_sha256),
        container_label,
        retail_provenance(install_sha256, container_label, 0, 0),
    )
    .expect("every claimed camera command is written by the real container");

    assert!(
        census.lines_walked() > 1_000,
        "the walk read the real container's scripts: {}",
        census.lines_walked()
    );
    for (command, expected) in RETAIL_CAMERA_COMMAND_COUNTS {
        assert_eq!(
            census.row(command).map(|row| row.count()),
            Some(expected),
            "{command}: the measured number of lines the real container stores with this command"
        );
    }
    assert_eq!(
        census.occurrences(),
        RETAIL_CAMERA_COMMAND_COUNTS
            .iter()
            .map(|(_, count)| count)
            .sum::<usize>(),
        "and the census total is the sum of its own rows, so nothing was counted twice"
    );
    // The original declares a camera named for its spyglass: the game's
    // magnified view exists as a *named camera object* in its own files, which is
    // a fact about the corpus and not a claim that this engine reproduces it.
    let spyglass = census
        .row("NewCamera")
        .expect("the camera-creation row exists")
        .occurrences()
        .iter()
        .any(|occurrence| {
            occurrence
                .arguments()
                .iter()
                .any(|argument| argument == "spyglass")
        });
    assert!(
        spyglass,
        "the real container creates a camera named `spyglass`: {:?}",
        census.row("NewCamera").map(|row| row.occurrences())
    );
    assert!(
        census.unconsumed().count() > 0,
        "at least one declared camera command has no consumer in this contract, which is the \
         coverage gap the stage records rather than hides"
    );

    // Completeness of the *claim list*, checked against the corpus rather than
    // asserted about it: the census records every distinct head the container
    // stores, so an eighth camera command the corpus held would be visible here
    // instead of silently missing from every coverage verdict.
    assert_eq!(
        census.heads().len(),
        RETAIL_COMMAND_HEADS,
        "the census walked every command head the real container stores"
    );
    let unclaimed_camera: Vec<(&str, usize)> = census
        .unclaimed_heads()
        .filter(|(head, _)| head.to_ascii_lowercase().contains("camera"))
        .collect();
    assert!(
        unclaimed_camera.is_empty(),
        "no camera command in the container is missing from the claim list: {unclaimed_camera:?}"
    );
    // And the claim list adds nothing the container does not hold, which is
    // what makes the seven rows above a census rather than a wish list.
    let claimed_not_stored: Vec<&str> = RETAIL_CAMERA_COMMAND_COUNTS
        .iter()
        .map(|(command, _)| *command)
        .filter(|command| {
            !census
                .heads()
                .keys()
                .any(|head| head.eq_ignore_ascii_case(command))
        })
        .collect();
    assert!(
        claimed_not_stored.is_empty(),
        "every claimed command is a head the container really stores: {claimed_not_stored:?}"
    );

    // The installation and canonical-content fingerprints the evidence report
    // carries, measured here so the report's `source` block is not typed in.
    let content_sha256 = cs_assets::install::content_fingerprint(&discovery.manifest);
    assert_eq!(install_sha256.to_hex().len(), 64);
    assert_eq!(content_sha256.to_hex().len(), 64);
    eprintln!(
        "F21-D retail: install {} content {}",
        install_sha256.to_hex(),
        content_sha256.to_hex()
    );
}

/// The real binding set checked against the **real** airframe archive: every
/// declared cockpit node, per airframe, resolved or reported by name.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f21_d_retail_every_declared_cockpit_binding_is_resolved_or_reported_by_name() {
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
    let claim = retail_cockpit_claim(install_sha256);
    let bindings = discover_cockpit_bindings(
        &decoded,
        &claim,
        container_label,
        retail_provenance(install_sha256, container_label, 12_850, 5_713),
    )
    .expect("the claimed script is the one the container holds");

    // The roster the bindings are checked against is F11-D2's discovery over the
    // same container: the eleven airframes the original's own build script
    // declares, with the roots it created.
    let roster = discover_airframe_roster(&decoded, &retail_roster_declarations(install_sha256))
        .expect("the real roster discovery reports findings, not a contradiction");
    assert_eq!(
        roster.airframe_count(),
        RETAIL_AIRFRAMES,
        "the measured number of declared airframes"
    );
    let airframes: Vec<CockpitAirframe> = roster
        .discovered()
        .iter()
        .map(CockpitAirframe::from)
        .collect();

    // The archive's catalog id, built through the production normalizer the
    // roster discovery itself uses, so the audit's container check compares two
    // ids that were derived the same way.
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
        "the node array the audit walks is the array the installation manifest declares for \
         zbd/planes.zbd"
    );
    let nodes = read_gamez_nodes(&mut ParseContext::with_defaults(archive_label), &planes)
        .expect("the airframe archive's node array reads");

    let report = audit_cockpit_coverage(&bindings, &airframes, &nodes, &archive_id)
        .expect("every declared root exists in the real archive");

    assert_eq!(report.rows().len(), RETAIL_AIRFRAMES);
    assert_eq!(
        report.declared_per_airframe(),
        bindings.bindings().len(),
        "every airframe is checked against the same declared binding set"
    );

    let mut resolved_total = 0_usize;
    let mut unresolved: Vec<(&str, &str, CockpitNodeCoverage)> = Vec::new();
    for row in report.rows() {
        assert!(
            row.subtree_nodes() > 1,
            "the {} subtree really holds nodes: {}",
            row.root(),
            row.subtree_nodes()
        );
        resolved_total += row.bound();
        for binding in row
            .bindings()
            .iter()
            .filter(|binding| !binding.coverage().is_bound())
        {
            unresolved.push((row.airframe(), binding.node(), binding.coverage().clone()));
        }
    }
    assert!(
        resolved_total > 0,
        "at least one declared cockpit node resolves to real geometry, or the audit has found \
         nothing at all"
    );
    // The exact resolution counts the finding quotes, pinned here so the
    // published table is a measurement rather than a transcription of one run:
    // 220 declared (airframe, binding) pairs, 176 resolved, 44 unresolved, and
    // the same sixteen resolved per aircraft on all eleven.
    assert_eq!(
        resolved_total + unresolved.len(),
        report.declared_per_airframe() * report.rows().len(),
        "every declared pair is either resolved or reported unresolved"
    );
    assert_eq!(resolved_total, 176, "the measured number of resolved pairs");
    assert_eq!(
        unresolved.len(),
        44,
        "the measured number of unresolved pairs"
    );
    let per_airframe: Vec<usize> = report
        .rows()
        .iter()
        .map(AirframeCockpitCoverage::bound)
        .collect();
    assert_eq!(
        per_airframe,
        vec![16; RETAIL_AIRFRAMES],
        "and each airframe resolves the same sixteen of its twenty"
    );
    // The unresolved set is the same four container-shaped names everywhere,
    // each in one of the two named states — the audit is not losing a binding
    // that resolved for one aircraft and not another.
    let mut unresolved_names: BTreeSet<&str> = BTreeSet::new();
    for row in report.rows() {
        // In the declaring script's order, not sorted: the audit reports what it
        // walked, and re-ordering it here would hide a change in that walk.
        assert_eq!(
            row.unresolved()
                .iter()
                .map(|binding| binding.node())
                .collect::<Vec<_>>(),
            ["gungauge", "4char_ammo", "missilegauge", "6char_type"],
            "{} resolves everything except the same four container-shaped names",
            row.airframe()
        );
        for binding in row.unresolved() {
            unresolved_names.insert(binding.node());
            assert!(
                matches!(
                    binding.coverage(),
                    CockpitNodeCoverage::NoMesh { .. } | CockpitNodeCoverage::Ambiguous { .. }
                ),
                "an unresolved binding is one of the two named states, got {:?}",
                binding.coverage()
            );
        }
    }
    assert_eq!(unresolved_names.len(), 4);
    eprintln!(
        "F21-D retail: {}/{} cockpit bindings resolved across {} airframes; {} unresolved",
        resolved_total,
        report.declared_per_airframe() * report.rows().len(),
        report.rows().len(),
        unresolved.len()
    );
    for (airframe, node, coverage) in &unresolved {
        eprintln!("F21-D retail: unresolved {airframe} {node} -> {coverage:?}");
    }

    // Every unresolved binding is one of the three named states, and every
    // resolved one names a node the archive really holds at that index. Whether
    // the mesh slot the node associates holds geometry is the mesh section's
    // answer, so it is checked here against the real mesh array rather than
    // assumed: `read_gamez_meshes` over the same file.
    let meshes = cs_formats::gamez::read_gamez_meshes(
        &mut ParseContext::with_defaults(archive_label),
        archive_label,
        &planes,
    )
    .expect("the archive's mesh section reads");
    let mut checked_slots = 0_usize;
    for (_, _, mesh_index) in report.drawable_meshes() {
        let slot = meshes
            .meshes
            .get(mesh_index as usize)
            .unwrap_or_else(|| panic!("mesh {mesh_index} is inside the archive's mesh array"));
        assert!(
            slot.is_some(),
            "mesh {mesh_index} is a stored mesh, not an empty stub slot: a node's `mesh_index` \
             is an association into the mesh array, and this audit's `Bound` verdict claims the \
             association, so a stub would mean the verdict is about nothing drawable"
        );
        checked_slots += 1;
    }
    assert_eq!(
        checked_slots, resolved_total,
        "every resolved pair's mesh slot was checked against the archive's own mesh array"
    );
    for row in report.rows() {
        for binding in row.bindings() {
            match binding.coverage() {
                CockpitNodeCoverage::Bound {
                    node_index,
                    mesh_index,
                } => {
                    let node = nodes
                        .get(*node_index)
                        .expect("the node index is in the array");
                    assert_eq!(
                        node.name,
                        binding.node(),
                        "the binding names the node it resolved"
                    );
                    assert_eq!(
                        node.info.mesh_index,
                        i32::try_from(*mesh_index).expect("a small index"),
                        "and the mesh index is the node's own stored association"
                    );
                }
                other => assert!(
                    matches!(
                        other,
                        CockpitNodeCoverage::Absent
                            | CockpitNodeCoverage::Ambiguous { .. }
                            | CockpitNodeCoverage::NoMesh { .. }
                    ),
                    "a binding is bound or one of the three named states, got {other:?}"
                ),
            }
        }
    }

    // And the pilot's eye is undeclared however much of the cockpit resolved.
    assert_eq!(report.eye(), CockpitEyeCoverage::Undeclared);
    // No airframe resolves all twenty, and the report says so rather than
    // reporting a partial result as a complete one.
    assert_eq!(
        report.complete().count(),
        0,
        "every one of the eleven airframes carries four container-shaped \
         bindings, so none of them resolves all twenty"
    );
    assert!(
        !report.is_complete(),
        "the report's own completeness verdict follows from its rows"
    );

    write_coverage_census(
        &report,
        &discover_view_controls(
            &decode_interp(&mut ParseContext::with_defaults(container_label), &bytes)
                .expect("re-reads"),
            &retail_view_claims(install_sha256),
            container_label,
            retail_provenance(install_sha256, container_label, 0, 0),
        )
        .expect("the same census the other retail test asserts"),
        &bindings,
        install_sha256,
        cs_assets::install::content_fingerprint(&discovery.manifest),
        resolved_total,
        unresolved.len(),
    );
}

/// Writes the derived coverage census under `CS_EVIDENCE_DIR` when the evidence
/// run sets it, so the acceptance report can carry this run's own numbers
/// instead of only the log lines they are printed on.
///
/// Outside an evidence run the variable is absent and nothing is written: the
/// test asserts the same numbers either way. The file carries the candidate tree
/// it was produced from, which the harness checks against the commit it reports
/// on, so a census left over from an earlier commit is refused rather than
/// reused.
///
/// What it holds is **derived** data: counts, stored node names, node and mesh
/// indices, byte offsets and digests. No original display text, no stored
/// geometry and no decompiled code, and nothing the log does not already state.
fn write_coverage_census(
    report: &cs_app::camera::coverage::CockpitCoverageReport,
    census: &cs_app::camera::coverage::ViewControlCensus,
    bindings: &cs_app::camera::coverage::CockpitBindingDiscovery,
    install_sha256: ContentHash,
    content_sha256: ContentHash,
    resolved: usize,
    unresolved: usize,
) {
    let Ok(dir) = std::env::var("CS_EVIDENCE_DIR") else {
        return;
    };
    let root = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .expect("git runs")
        .stdout;
    let root = String::from_utf8_lossy(&root).trim().to_owned();
    let directory = if Path::new(&dir).is_absolute() {
        PathBuf::from(&dir)
    } else {
        Path::new(&root).join(&dir)
    };
    std::fs::create_dir_all(&directory).expect("the evidence directory is writable");
    let tree = std::process::Command::new("git")
        .args(["rev-parse", "HEAD^{tree}"])
        .current_dir(&root)
        .output()
        .expect("git runs")
        .stdout;
    let tree = String::from_utf8_lossy(&tree).trim().to_owned();

    let commands: Vec<String> = census
        .rows()
        .iter()
        .map(|row| {
            format!(
                "{{\"command\":{},\"effect\":{},\"coverage\":{},\"occurrences\":{},\
                  \"scripts\":[{}],\"first_offset\":{}}}",
                quote(row.command()),
                quote(row.effect().label()),
                quote(row.coverage().label()),
                row.count(),
                row.scripts()
                    .iter()
                    .map(|script| quote(script))
                    .collect::<Vec<_>>()
                    .join(","),
                row.occurrences().first().map_or(0, |row| row.offset()),
            )
        })
        .collect();
    let declared: Vec<String> = bindings
        .bindings()
        .iter()
        .map(|binding| {
            format!(
                "{{\"node\":{},\"occurrences\":{},\"first_at\":{},\"span\":{}}}",
                quote(binding.node()),
                binding.occurrences(),
                binding.first_at(),
                span_json(binding.provenance()),
            )
        })
        .collect();
    let airframes: Vec<String> = report
        .rows()
        .iter()
        .map(|row| {
            let bindings: Vec<String> = row
                .bindings()
                .iter()
                .map(|binding| {
                    format!(
                        "{{\"node\":{},\"state\":{},\"node_index\":{},\"mesh_index\":{}}}",
                        quote(binding.node()),
                        quote(coverage_state(binding.coverage())),
                        binding
                            .coverage()
                            .node_index()
                            .map_or("null".to_owned(), |index| index.to_string()),
                        binding
                            .coverage()
                            .mesh_index()
                            .map_or("null".to_owned(), |index| index.to_string()),
                    )
                })
                .collect();
            format!(
                "{{\"airframe\":{},\"root\":{},\"root_ref\":{},\"subtree_nodes\":{},\
                  \"bound\":{},\"declared\":{},\"bindings\":[{}]}}",
                quote(row.airframe()),
                quote(row.root()),
                quote(row.root_ref()),
                row.subtree_nodes(),
                row.bound(),
                row.bindings().len(),
                bindings.join(","),
            )
        })
        .collect();

    // Built before the outer format so no `format!` is nested inside another
    // one's argument list (a clippy `format_in_format_args` failure) and so each
    // block reads as one value.
    let camera_commands = format!(
        "{{\"lines_walked\":{},\"distinct_command_heads\":{},\"unclaimed_camera_commands\":{},\
          \"occurrences\":{},\"unconsumed_occurrences\":{},\"rows\":[{}]}}",
        census.lines_walked(),
        census.heads().len(),
        census
            .unclaimed_heads()
            .filter(|(head, _)| head.to_ascii_lowercase().contains("camera"))
            .count(),
        census.occurrences(),
        census.unconsumed().count(),
        commands.join(",")
    );
    let cockpit_script = format!(
        "{{\"script\":{},\"lines_walked\":{},\"clean\":{},\"findings\":{}}}",
        quote(bindings.script()),
        bindings.lines_walked(),
        bindings.is_clean(),
        bindings.findings().len()
    );
    let document = format!(
        "{{\"schema\":\"cs-f21-d-view-cockpit-coverage/1\",\"candidate_tree\":{},\
          \"install_sha256\":{},\"content_sha256\":{},\"interp_container\":{},\
          \"airframe_archive\":{},\"camera_commands\":{},\"cockpit_script\":{},\
          \"declared_bindings\":[{}],\"airframes\":[{}],\"resolved_pairs\":{},\
          \"unresolved_pairs\":{},\"eye_placement\":{},\"eye_placement_resolves_in\":{},\
          \"view_control_runtime\":{},\"authored_camera_clips\":{}}}\n",
        quote(&tree),
        quote(&install_sha256.to_hex()),
        quote(&content_sha256.to_hex()),
        quote(bindings.container()),
        quote(report.archive()),
        camera_commands,
        cockpit_script,
        declared.join(","),
        airframes.join(","),
        resolved,
        unresolved,
        quote(report.eye().label()),
        quote(
            "an owner-supplied original run that shows a pilot's eye placement, or a bounded \
             static analysis of the packed executable; no readable file declares it"
        ),
        quote(
            "no shipped file holds a key/mouse/joystick to command map (F22-H measured the label \
             vocabulary and found the bindings themselves native)"
        ),
        quote(
            "the world's cam_anim carriers are fingerprinted by F20-D and the .zan/.zrd clips they \
             reference are still an undecoded layout (F20-C/F40 own that)"
        ),
    );
    let path = directory.join("view-cockpit-coverage.json");
    std::fs::write(&path, &document)
        .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
    println!("wrote {}", path.display());
}

/// The audit's own name for one binding state.
fn coverage_state(coverage: &CockpitNodeCoverage) -> &'static str {
    match coverage {
        CockpitNodeCoverage::Bound { .. } => "bound",
        CockpitNodeCoverage::Ambiguous { .. } => "ambiguous",
        CockpitNodeCoverage::Absent => "absent",
        CockpitNodeCoverage::NoMesh { .. } => "no_mesh",
    }
}

/// The source span a measured row carries, or `null`.
fn span_json(provenance: &Provenance) -> String {
    match &provenance.source {
        Some(span) => format!(
            "{{\"container\":{},\"offset\":{},\"length\":{}}}",
            quote(span.container_path()),
            span.offset(),
            span.length()
        ),
        None => "null".to_owned(),
    }
}

/// A JSON string literal: quoted and escaped.
fn quote(value: &str) -> String {
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

/// One bound cockpit node's **real** geometry, captured on a real adapter.
///
/// This is the stage's `gpu` capability over `retail` bytes: the mesh index came
/// out of the coverage audit above, the mesh is the production render mesh the
/// airframe archive's own mesh section produced, and the capture is refused
/// rather than written when the frame comes back uniform.
#[test]
#[ignore = "requires CS_GAME_DIR and a GPU adapter"]
fn accept_f21_d_retail_a_bound_cockpit_mesh_draws_a_measured_frame_on_the_gpu() {
    use cs_app::world::gpu_capture::{CaptureRequest, capture_world_mesh};
    use cs_content::mesh::RenderMesh;
    use cs_formats::gamez::read_gamez_meshes;
    use cs_formats::io::ParseContext;

    let game_dir = retail_dir();
    let archive_label = "ZBD/planes.zbd";
    let bytes = std::fs::read(game_dir.join("ZBD").join("planes.zbd"))
        .expect("the shared airframe archive must be there");
    let meshes = read_gamez_meshes(
        &mut ParseContext::with_defaults(archive_label),
        archive_label,
        &bytes,
    )
    .expect("the archive's mesh section reads");

    // One node of the archive whose own stored name is a declared cockpit
    // binding, with a mesh it associates. Chosen from the bytes rather than from
    // a hardcoded index, so the test cannot pass on an index that has moved.
    let nodes = cs_formats::gamez::read_gamez_nodes(
        &mut ParseContext::with_defaults(archive_label),
        &bytes,
    )
    .expect("the archive's node array reads");
    let declared = cockpit_binding_names();
    let mut chosen: Option<(String, u32, u32)> = None;
    for node in &nodes.nodes {
        if node.info.mesh_index < 0 || !declared.contains(node.name.as_str()) {
            continue;
        }
        let mesh_index = node.info.mesh_index as u32;
        let Some(slot) = meshes
            .meshes
            .get(mesh_index as usize)
            .and_then(Option::as_ref)
        else {
            continue;
        };
        let Ok(render) = RenderMesh::from_stored_groups(&slot.mesh, &slot.material_groups) else {
            continue;
        };
        if render.triangles().is_empty() {
            continue;
        }
        chosen = Some((
            node.name.clone(),
            mesh_index,
            render.triangles().len() as u32,
        ));
        // One is enough: the point is that a bound cockpit node's own geometry
        // draws, and eight captures cost eight driver round trips.
        break;
    }
    let (name, mesh_index, triangles) =
        chosen.expect("the archive holds a declared cockpit node with real geometry");
    eprintln!("F21-D retail GPU: capturing {name} (mesh {mesh_index}, {triangles} triangles)");

    let slot = meshes
        .meshes
        .get(mesh_index as usize)
        .and_then(Option::as_ref)
        .expect("the chosen mesh is in the section");
    let render = RenderMesh::from_stored_groups(&slot.mesh, &slot.material_groups)
        .expect("the chosen mesh builds a render mesh");

    let png = evidence_dir().join(format!("f21-d-cockpit-{name}-{mesh_index}.png"));
    let capture = capture_world_mesh(&CaptureRequest {
        group: "zbd/planes.zbd",
        mesh_index,
        render: &render,
        unknowns: &[],
        png: &png,
    })
    .expect("a real adapter draws the bound cockpit mesh and returns a measured capture");

    assert!(
        capture.distinct_luminance > 1,
        "a frame with one luminance level is the background alone: {capture:?}"
    );
    assert!(
        capture.covered_pixels > 0,
        "nothing reached the frame: {capture:?}"
    );
    assert!(capture.drew_geometry());
    assert_eq!(capture.mesh_index, mesh_index);
    assert_eq!(
        capture.triangles as u32, triangles,
        "the triangles submitted are the ones the stored mesh holds"
    );
    let written = std::fs::read(&png).expect("the PNG is on disk");
    assert_eq!(cs_assets::install::sha256(&written), capture.png_sha256);
    assert_eq!(written.len() as u64, capture.png_bytes);
    eprintln!(
        "F21-D retail GPU: {} drew {} of {} pixels ({} per mille), adapter {:?}, png {} bytes",
        name,
        capture.covered_pixels,
        capture.width as usize * capture.height as usize,
        capture.covered_permille,
        capture.adapter,
        capture.png_bytes
    );
}

// ------------------------------------------------- the retail fixtures ----

/// How many airframes the real container declares.
const RETAIL_AIRFRAMES: usize = 11;

/// How many lines the real container stores for each claimed camera command.
///
/// Measured by the retail census above, which fails if any count moves. The
/// horizon levelling is the interesting one: **every** world group's `load.gw`
/// writes it, for the default camera and for the spyglass camera.
const RETAIL_CAMERA_COMMAND_COUNTS: [(&str, usize); 7] = [
    ("NewCamera", 2),
    ("CameraSetActive", 2),
    ("CameraSetWorld", 2),
    ("CameraSetWindow", 2),
    ("CameraSetHorizon", 16),
    ("CameraSetHorizonXZ", 4),
    ("CameraSetObjectHSETest", 1),
];

/// How many distinct command heads the real container stores across its 98
/// scripts.
///
/// Measured by the retail census above, which fails if the count moves. It is
/// here so the claim list's **completeness** is checkable: the census records
/// every head, and the retail test asserts that every head spelling `camera` is
/// claimed, so "seven camera commands" is a fact about the corpus rather than
/// about the seven spellings somebody happened to type.
const RETAIL_COMMAND_HEADS: usize = 85;

/// The distinct cockpit node names `support\cockpit.gw` binds, in first-use
/// order.
///
/// Transcribed from the real container and **pinned by the retail test above**,
/// which fails if the walk returns anything else. A transcription is not a
/// source of truth; it is the expected value the measurement is compared
/// against, so a reader who wants the real list has one that was compared.
const RETAIL_COCKPIT_BINDINGS: [&str; 20] = [
    "gungauge",
    "4char_ammo",
    "missilegauge",
    "ggindicator0",
    "ggindicator1",
    "ggindicator2",
    "ggindicator3",
    "mgindicator0",
    "mgindicator1",
    "mgindicator2",
    "mgindicator3",
    "mgindicator4",
    "mgindicator5",
    "mgindicator6",
    "mgindicator7",
    "6char_type",
    "rightwingdamage",
    "leftwingdamage",
    "taildamage",
    "nosedamage",
];

/// The declared cockpit names, as a set, for the GPU test's own lookup.
fn cockpit_binding_names() -> BTreeSet<&'static str> {
    RETAIL_COCKPIT_BINDINGS.into_iter().collect()
}

/// The claim over the real `support\cockpit.gw`, with the span it occupies.
///
/// The offsets and length are the decoder's own measurements for that script
/// (stored script offset 12 850, terminator past 18 563), and the claim's
/// provenance is the installation's own fingerprint, so every row the walk
/// reports points at real bytes of a real file.
fn retail_cockpit_claim(install_sha256: ContentHash) -> CockpitBindingClaim {
    CockpitBindingClaim::try_new(
        "support\\cockpit.gw",
        "FindNode",
        "FindSubNode",
        "%player_plane%",
        retail_provenance(install_sha256, "ZBD/interp.zbd", 12_850, 5_713),
    )
    .expect("the real claim is well formed")
}

/// The camera-command claims over the real container.
///
/// Seven spellings, the arities the corpus stores, and coverage judged against
/// this engine: `NewCamera`, `CameraSetActive` and `CameraSetWorld` have
/// consumers here, and `CameraSetWindow`, `CameraSetHorizon`,
/// `CameraSetHorizonXZ` and `CameraSetObjectHSETest` do not — each naming what
/// the camera contract would have to gain.
fn retail_view_claims(install_sha256: ContentHash) -> Vec<ViewCommandClaim> {
    let provenance = || retail_provenance(install_sha256, "ZBD/interp.zbd", 18_563, 1_012);
    let mut claims = Vec::new();
    for (command, arguments, effect, coverage) in [
        (
            "NewCamera",
            2_usize,
            ViewControlEffect::CreatesCamera,
            ViewControlCoverage::Consumed {
                operation: CameraOperation::AuthoredCamera,
            },
        ),
        (
            "CameraSetActive",
            2,
            ViewControlEffect::ActivatesCamera,
            ViewControlCoverage::Consumed {
                operation: CameraOperation::PlayerRig,
            },
        ),
        (
            "CameraSetWorld",
            2,
            ViewControlEffect::BindsCameraToWorld,
            ViewControlCoverage::Consumed {
                operation: CameraOperation::PlayerRig,
            },
        ),
        (
            "CameraSetWindow",
            2,
            ViewControlEffect::BindsCameraToWindow,
            ViewControlCoverage::Unconsumed {
                reason: "the camera contract has no viewport/window binding".to_owned(),
            },
        ),
        (
            "CameraSetHorizon",
            2,
            ViewControlEffect::LevelsHorizon,
            ViewControlCoverage::Unconsumed {
                reason: "no camera in this contract can level its up axis against a world horizon"
                    .to_owned(),
            },
        ),
        (
            "CameraSetHorizonXZ",
            2,
            ViewControlEffect::LevelsHorizonToZone,
            ViewControlCoverage::Unconsumed {
                reason: "no camera in this contract can level its up axis against a named zone"
                    .to_owned(),
            },
        ),
        (
            "CameraSetObjectHSETest",
            2,
            // Not a horizon operation: nothing in a readable file says what an
            // object hull-sensitivity test does to a camera, so it is reported
            // undetermined rather than filed under an operation nobody measured.
            ViewControlEffect::Undetermined,
            ViewControlCoverage::Unconsumed {
                reason: "the hull-sensitivity probe has no counterpart in this contract".to_owned(),
            },
        ),
    ] {
        claims.push(
            ViewCommandClaim::try_new(command, arguments, effect, coverage, provenance())
                .unwrap_or_else(|error| panic!("the real {command} claim is valid, got {error}")),
        );
    }
    claims
}

/// The roster declarations over the real container, so the coverage audit is
/// measured against F11-D2's discovered roster rather than a transcription.
fn retail_roster_declarations(install_sha256: ContentHash) -> RosterDeclarations {
    RosterDeclarations::new(vec![AirframeDeclaration {
        script: "support\\planes.gw".to_owned(),
        bind_command: "set".to_owned(),
        include_command: "source".to_owned(),
        write_command: "GameZWriteZBDFile".to_owned(),
        create_command: "NewObject3D".to_owned(),
        container_variable: "ZBDFile".to_owned(),
        root_variable: "planeOutput".to_owned(),
        model_variable: "planeInput".to_owned(),
        required_roles: vec![RosterRoleRule {
            role: cs_content::scene::PartRole::Cockpit,
            provenance: retail_provenance(install_sha256, "ZBD/interp.zbd", 187_904, 392),
        }],
        provenance: retail_provenance(install_sha256, "ZBD/interp.zbd", 37_838, 8_650),
    }])
    .expect("one declaration naming one script is valid")
}

/// Provenance over a real byte range of a real file.
fn retail_provenance(
    install_sha256: ContentHash,
    container: &str,
    offset: u64,
    length: u64,
) -> Provenance {
    let span = SourceSpan::new(install_sha256, container, None, offset, length, None)
        .expect("a span over real bytes is valid");
    Provenance::new(claim(), ClaimStatus::ObservedTool, Some(span))
        .expect("an observed_tool claim with a source span is valid")
}

/// The installation path, or a panic: a retail test that quietly passes without
/// the original data is a test that proves nothing.
fn retail_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR is set for a retail test; without it this test must fail"),
    )
}
