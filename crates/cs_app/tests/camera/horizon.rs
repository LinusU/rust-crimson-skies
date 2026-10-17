//! Task #541 acceptance tests: the measured
//! `CameraSetHorizon`/`CameraSetHorizonXZ` mechanism — named world scenery
//! re-anchored to the camera's position every render pass, never a lock of
//! the camera's up axis.
//!
//! Spec: `specs/F21-cameras-cockpit-views-and-spyglass.md`, stage
//! `### F21-D` (this task is its follow-up). Task test prefix:
//! `accept_t541_`. Findings:
//! `docs/findings/2026-10-11-t541-horizon-commands-anchor-scenery-to-camera.md`.
//!
//! The instruments under test are production code in `cs_app::camera::horizon`
//! — [`HorizonAnchorRule`] (which translation each command's handler installs),
//! [`HorizonBinding`] (the bound state: which named node, under which rule),
//! [`horizon_bindings`] (the binding set a measured census declares) and
//! [`mechanism_source`] (the static-analysis spans over the owner-supplied
//! decrypted executable the whole claim stands on). No test here carries its
//! own disassembler: the executable-image test asserts *bytes* — the stored
//! command names, the two field stores and the apply routine's call targets —
//! against the spans the production record names.
//!
//! The suite is split the way the capabilities are:
//!
//! * six tests read **no** original data and are not ignored: they pin the
//!   contract on authored containers and on the mechanism's own refusal of an
//!   image it was not measured in;
//! * two tests need the installation (`#[ignore = "requires CS_GAME_DIR"]`):
//!   the real loading-script census becomes twenty real bindings, and the
//!   world archives are checked for where the bound scenery actually lives;
//! * one test needs the owner-supplied decrypted executable
//!   (`#[ignore = "requires CS_ENGINE_IMAGE"]`): the mechanism's spans must
//!   cover the measured instructions, byte for byte.
//!
//! What no test asserts, because nothing measured it: where the original
//! instantiates `zone2_cloud_floor`/`moon_reflection` (the retail world
//! archives do not store them, and the test pins that absence rather than
//! guessing a source), and the original's runtime draw — byte inspection of
//! the owner-supplied image is static analysis, never a run of the original.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cs_app::camera::{
    CameraOperation, CameraPose, HorizonAnchorRule, HorizonBinding, HorizonError, HorizonMechanism,
    HorizonMechanismError, ViewCommandClaim, ViewControlCensus, ViewControlCoverage,
    ViewControlEffect, discover_view_controls, horizon_bindings, mechanism_source,
};
use cs_content::coordinates::EngineImage;
use cs_formats::interp::decode_interp;
use cs_formats::io::ParseContext;
use cs_types::asset_id::SourceSpan;
use cs_types::content::{Origin, Provenance};
use cs_types::evidence::{ClaimId, ClaimStatus, ContentHash};
use cs_types::space::{Quaternion, Radians, UnitVec3};

use crate::camera::coverage::{
    FIXTURE, container, decode, line, observed, quote, retail_dir, retail_provenance,
};

// ------------------------------------------------------------- fixtures ---

/// The claim id every authored fixture value is recorded under.
fn claim() -> ClaimId {
    ClaimId::new("t541.horizon-fixture").expect("a valid claim id")
}

/// Designed provenance for a fixture value.
fn designed() -> Provenance {
    Provenance::designed(claim())
}

/// A world position from three metres.
fn position([x, y, z]: [f64; 3]) -> cs_types::space::WorldPosition {
    cs_types::space::WorldPosition::try_new([x, y, z]).expect("the fixture position is finite")
}

/// The claims a census of the two measured horizon commands needs — the same
/// measured effect and consumer the coverage suite's claim list carries, kept
/// here so this suite's claim is the pair the task measured and nothing else.
fn horizon_claims(provenance: Provenance) -> Vec<ViewCommandClaim> {
    [
        (
            "CameraSetHorizon",
            ViewControlEffect::AnchorsSceneryToCamera,
        ),
        (
            "CameraSetHorizonXZ",
            ViewControlEffect::AnchorsSceneryToCameraLaterally,
        ),
    ]
    .into_iter()
    .map(|(command, effect)| {
        ViewCommandClaim::try_new(
            command,
            2,
            effect,
            ViewControlCoverage::Consumed {
                operation: CameraOperation::SceneryAnchors,
            },
            provenance.clone(),
        )
        .expect("the measured claims are well formed")
    })
    .collect()
}

/// The authored loading script these tests census, in the real corpus's own
/// shape: each camera is found, then the horizon container and a lateral
/// anchor are bound to it, one pair per camera.
///
/// ```text
/// FindNode camera1
/// CameraSetHorizon   horizon
/// CameraSetHorizonXZ zone2_cloud_floor
/// FindNode spyglass
/// CameraSetHorizon   horizon
/// CameraSetHorizonXZ moon_reflection
/// ```
fn synthetic_load_script() -> Vec<u8> {
    container(&[(
        b"support\\c1\\load.gw",
        vec![
            line(&[b"FindNode", b"camera1"]),
            line(&[b"CameraSetHorizon", b"horizon"]),
            line(&[b"CameraSetHorizonXZ", b"zone2_cloud_floor"]),
            line(&[b"FindNode", b"spyglass"]),
            line(&[b"CameraSetHorizon", b"horizon"]),
            line(&[b"CameraSetHorizonXZ", b"moon_reflection"]),
        ],
    )])
}

/// A binding over designed provenance for the refusals below.
fn fixture_binding(command: &str, node: &str) -> Result<HorizonBinding, HorizonError> {
    HorizonBinding::for_command(command, node, Origin::Designed, designed())
}

// ---------------------------------------------------- the command's rule ---

/// The dispatch's `strnicmp` is case-insensitive, and the command that names
/// a rule is the stored **whole** name — `CameraSetHorizonXZ` is the lateral
/// binding and is never read as the 16-byte `CameraSetHorizon` it prefixes,
/// because the walk records the stored head and the rule lookup compares the
/// whole spelling, matching the dispatch's XZ-first order.
#[test]
fn accept_t541_the_two_measured_commands_name_their_rules() {
    assert_eq!(
        HorizonAnchorRule::for_command("CameraSetHorizon"),
        Some(HorizonAnchorRule::AllAxes),
        "CameraSetHorizon's handler stores the all-axes slot at +0x8"
    );
    assert_eq!(
        HorizonAnchorRule::for_command("CameraSetHorizonXZ"),
        Some(HorizonAnchorRule::KeepOwnHeight),
        "CameraSetHorizonXZ's handler stores the keep-height slot at +0xc"
    );
    for spelling in [
        "camerasethorizon",
        "CAMERASETHORIZON",
        "CameraSetHorizonXZ",
        "camerasethorizonxz",
    ] {
        assert!(
            HorizonAnchorRule::for_command(spelling).is_some(),
            "the dispatch compares with strnicmp: {spelling}"
        );
    }
    for spelling in [
        "CameraSetHorizonXY",
        "CameraSetHorizonXYZ",
        "CameraSetRoll",
        "FindNode",
        "",
    ] {
        assert_eq!(
            HorizonAnchorRule::for_command(spelling),
            None,
            "not one of the two measured commands: {spelling:?}"
        );
    }
    assert_eq!(
        HorizonAnchorRule::AllAxes.to_string(),
        "all_axes",
        "the report label is stable"
    );
    assert_eq!(
        HorizonAnchorRule::KeepOwnHeight.to_string(),
        "keep_own_height"
    );
}

/// The measured per-pass write, as a value: `0x4d6390` hands the bound node's
/// local translate slot to `0x4d1d50` with the camera's world translation —
/// all three axes for the `+0x8` node, and `(camera.x, node.y, camera.z)` for
/// the `+0xc` node, whose own stored height `0x4d1cc0` reads back.
#[test]
fn accept_t541_the_measured_write_anchors_the_node_to_the_camera() {
    let camera = position([12.0, 34.0, -56.0]);
    let node = position([1.0, 2.0, 3.0]);

    assert_eq!(
        HorizonAnchorRule::AllAxes.anchored_translation(camera, node),
        camera,
        "the horizon container is re-anchored to the camera on every axis"
    );
    assert_eq!(
        HorizonAnchorRule::KeepOwnHeight.anchored_translation(camera, node),
        position([12.0, 2.0, -56.0]),
        "the lateral anchor keeps the node's own stored height and slides \
         under the camera's x/z"
    );

    // The same write through the binding record, under the fixture's own
    // values: the binding's rule is what the handler installed.
    let all = fixture_binding("CameraSetHorizon", "horizon").expect("a horizon binding is valid");
    assert_eq!(
        all.anchored_translation(camera, node),
        camera,
        "the binding applies its own rule"
    );
    let lateral = fixture_binding("CameraSetHorizonXZ", "zone2_cloud_floor")
        .expect("a lateral binding is valid");
    assert_eq!(
        lateral.anchored_translation(camera, node),
        position([12.0, 2.0, -56.0])
    );
}

/// A camera's rotation is **not an input** to the measured write: two cameras
/// at the same spot with different attitudes anchor the scenery identically.
/// The apply routine reads only the camera's world translation
/// (`camdata+0x2c..+0x34`), and this pin is what falsifies F21-D's earlier
/// "levels the camera's up axis" classification — a basis would change the
/// output, and nothing here reads one.
#[test]
fn accept_t541_the_binding_takes_a_position_never_a_rotation() {
    let spot = position([120.0, 40.0, -8.0]);
    let banked = CameraPose::new(
        spot,
        Quaternion::from_axis_angle(UnitVec3::FORWARD, Radians(0.7))
            .expect("a banked attitude is a rotation"),
    );
    let pitched = CameraPose::new(
        spot,
        Quaternion::from_axis_angle(
            UnitVec3::try_new([1.0, 0.0, 0.0]).expect("+X is a unit axis"),
            Radians(-0.4),
        )
        .expect("a pitched attitude is a rotation"),
    );
    let binding =
        fixture_binding("CameraSetHorizon", "horizon").expect("a horizon binding is valid");
    let node = position([0.0, 0.0, 0.0]);

    let under_banked = binding.anchored_translation(banked.position(), node);
    let under_pitched = binding.anchored_translation(pitched.position(), node);
    assert_eq!(
        under_banked, under_pitched,
        "the write reads the camera's position and nothing else: two \
         attitudes at the same spot anchor the scenery identically"
    );
    assert_eq!(
        under_banked, spot,
        "and what it writes is the camera's position, into the node's slot"
    );
    // The pose the write read is unchanged: the measured direction is
    // scenery <- camera, one way only.
    assert_eq!(banked.position(), spot);
    assert_eq!(pitched.position(), spot);
}

// ------------------------------------------------------------ the binding ---

/// A binding is the state the command's handler installs: the named node and
/// the measured rule, with where the declaration was measured — and the
/// declared name is validated the way every declared spelling in the module
/// is.
#[test]
fn accept_t541_a_binding_records_node_rule_origin_and_provenance() {
    let binding = fixture_binding("CameraSetHorizonXZ", "moon_reflection")
        .expect("the fixture binding is valid");
    assert_eq!(binding.node(), "moon_reflection");
    assert_eq!(binding.rule(), HorizonAnchorRule::KeepOwnHeight);
    assert_eq!(binding.origin(), &Origin::Designed);
    assert_eq!(
        binding.provenance().class,
        ClaimStatus::Designed,
        "an authored binding says it was authored"
    );

    assert_eq!(
        fixture_binding("CameraSetHorizon", ""),
        Err(HorizonError::DeclaredName {
            field: "node",
            len: 0
        }),
        "an empty node name is refused"
    );
    let too_long = "n".repeat(65);
    assert_eq!(
        fixture_binding("CameraSetHorizon", &too_long),
        Err(HorizonError::DeclaredName {
            field: "node",
            len: 65
        }),
        "a node name past the declared bound is refused"
    );
    assert_eq!(
        fixture_binding("CameraSetRoll", "horizon"),
        Err(HorizonError::NotAHorizonCommand {
            command: "CameraSetRoll".to_owned()
        }),
        "a command the analysis did not measure installs no binding"
    );
}

/// The binding set a measured census declares: one binding per horizon
/// occurrence, in container order, each carrying the rule its command
/// installs and the provenance the walk measured — and a census whose
/// container binds no horizon yields no bindings.
#[test]
fn accept_t541_bindings_derive_from_a_measured_census() {
    let bytes = synthetic_load_script();
    let census = discover_view_controls(
        &decode(&bytes),
        &horizon_claims(observed(0, 16)),
        FIXTURE,
        observed(0, 16),
    )
    .expect("the authored container stores both claimed commands");

    let bindings = horizon_bindings(&census, &Origin::SyntheticFixture)
        .expect("every measured occurrence carries a node to bind");
    let named: Vec<(&str, HorizonAnchorRule)> = bindings
        .iter()
        .map(|binding| (binding.node(), binding.rule()))
        .collect();
    assert_eq!(
        named,
        [
            ("horizon", HorizonAnchorRule::AllAxes),
            ("horizon", HorizonAnchorRule::AllAxes),
            ("zone2_cloud_floor", HorizonAnchorRule::KeepOwnHeight),
            ("moon_reflection", HorizonAnchorRule::KeepOwnHeight),
        ],
        "one binding per occurrence, in claim order then the script's own \
         order — the census's own occurrence order"
    );
    for binding in &bindings {
        assert_eq!(
            binding.origin(),
            &Origin::SyntheticFixture,
            "a fixture census yields fixture bindings"
        );
        assert_eq!(
            binding.provenance(),
            &observed(0, 16),
            "the binding carries the provenance the walk measured, not a \
             fabricated one"
        );
    }

    // A census whose container writes no horizon command binds nothing —
    // claimed here over the `FindNode` lines the same container stores.
    let finder_claim = ViewCommandClaim::try_new(
        "FindNode",
        2,
        ViewControlEffect::Undetermined,
        ViewControlCoverage::Unconsumed {
            reason: "not a camera operation".to_owned(),
        },
        observed(0, 16),
    )
    .expect("the finder claim is well formed");
    let census = discover_view_controls(&decode(&bytes), &[finder_claim], FIXTURE, observed(0, 16))
        .expect("the container stores FindNode lines");
    assert!(
        horizon_bindings(&census, &Origin::SyntheticFixture)
            .expect("the census reads")
            .is_empty(),
        "a census without the measured commands yields no bindings"
    );
}

// ------------------------------------------------- the mechanism's spans ---

/// The mechanism claim stands on one image: an `EngineImage` that is not the
/// measured owner-supplied executable is refused, because spans over other
/// bytes cannot back what was measured here.
#[test]
fn accept_t541_the_mechanism_refuses_an_image_it_was_not_measured_in() {
    let wrong =
        ContentHash::from_hex("1111111111111111111111111111111111111111111111111111111111111111")
            .expect("sixty-four nibbles are a digest");
    let image = EngineImage {
        path: PathBuf::from("some/other/image.exe"),
        bytes: vec![0_u8; 16],
        digest: wrong,
    };
    assert_eq!(
        mechanism_source(&image),
        Err(HorizonMechanismError::DigestMismatch { found: wrong }),
        "only the measured image's spans can back the mechanism claim"
    );
}

// -------------------------------------------------------- retail census ---

/// The real loading-script container declares twenty bindings: `horizon`
/// under [`HorizonAnchorRule::AllAxes`] for both cameras of every world
/// group, and one lateral anchor under [`HorizonAnchorRule::KeepOwnHeight`]
/// per camera where the script writes one — `zone2_cloud_floor` in C1's two
/// cameras and `moon_reflection` in C1B's two.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t541_retail_the_loading_scripts_bind_twenty_named_scenery_nodes() {
    let game_dir = retail_dir();
    let discovery = cs_assets::install::discover(&game_dir).expect("production discovery reads it");
    let install_sha256 = cs_assets::install::fingerprint(&discovery.manifest);
    let container_label = "ZBD/interp.zbd";
    let bytes = std::fs::read(game_dir.join("ZBD").join("interp.zbd"))
        .expect("the loading-script container must be there");
    // The bytes this census walks are the bytes the installation's own
    // manifest declares for the container.
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
        "the census describes this installation's own container"
    );
    let decoded = decode_interp(&mut ParseContext::with_defaults(container_label), &bytes)
        .expect("the real container decodes");

    let install_span = SourceSpan::new(
        install_sha256,
        container_label,
        None,
        0,
        bytes.len() as u64,
        None,
    )
    .expect("a span over the real container is valid");
    let install_origin = Origin::Installation {
        source: install_span.clone(),
    };
    let claim_provenance = retail_provenance(install_sha256, container_label, 18_563, 1_012);
    let census = discover_view_controls(
        &decoded,
        &horizon_claims(claim_provenance.clone()),
        container_label,
        claim_provenance.clone(),
    )
    .expect("the real container stores both measured commands");

    // The census numbers themselves, which F21-D measured: sixteen horizon
    // bindings — two cameras in each of the eight world groups — and four
    // lateral bindings.
    assert_eq!(
        census.row("CameraSetHorizon").map(|row| row.count()),
        Some(16)
    );
    assert_eq!(
        census.row("CameraSetHorizonXZ").map(|row| row.count()),
        Some(4)
    );
    let scripts: BTreeMap<&str, usize> = census
        .row("CameraSetHorizon")
        .expect("the horizon row exists")
        .occurrences()
        .iter()
        .fold(BTreeMap::new(), |mut counts, occurrence| {
            *counts.entry(occurrence.script()).or_default() += 1;
            counts
        });
    assert_eq!(
        scripts.len(),
        8,
        "eight world groups each write the binding: {scripts:?}"
    );
    assert!(
        scripts
            .iter()
            .all(|(script, count)| *count == 2 && script.ends_with("load.gw")),
        "each world group's load.gw binds it twice — camera1 and spyglass: \
         {scripts:?}"
    );

    let bindings = horizon_bindings(&census, &install_origin)
        .expect("every measured occurrence carries a node to bind");
    assert_eq!(bindings.len(), 20, "sixteen plus four measured occurrences");

    let mut all_axes = 0_usize;
    let mut lateral: BTreeMap<&str, usize> = BTreeMap::new();
    for binding in &bindings {
        match binding.rule() {
            HorizonAnchorRule::AllAxes => {
                all_axes += 1;
                assert_eq!(
                    binding.node(),
                    "horizon",
                    "every all-axes binding names the sky container"
                );
            }
            HorizonAnchorRule::KeepOwnHeight => {
                *lateral.entry(binding.node()).or_default() += 1;
            }
        }
        let Origin::Installation { source } = binding.origin() else {
            panic!("a binding over the real container is installation data")
        };
        assert_eq!(
            source.install_sha256(),
            install_sha256,
            "the binding's origin names this installation"
        );
        assert_eq!(source.container_path(), container_label);
        assert_eq!(
            binding.provenance().class,
            ClaimStatus::ObservedTool,
            "the binding says how it was measured"
        );
        let span = binding
            .provenance()
            .source
            .as_ref()
            .expect("a measured binding names the bytes it came from");
        assert_eq!(span.install_sha256(), install_sha256);
        assert_eq!(span.container_path(), container_label);
    }
    assert_eq!(all_axes, 16);
    assert_eq!(
        lateral,
        BTreeMap::from([("moon_reflection", 2), ("zone2_cloud_floor", 2)]),
        "C1 binds the cloud floor and C1B the moon reflection, once per \
         camera each"
    );

    write_binding_census(
        &census,
        install_sha256,
        cs_assets::install::content_fingerprint(&discovery.manifest),
    );
}

/// The node the scripts bind is real scenery: `horizon` is an `object3d`
/// record parented to `world1` in **every** world group's `gamez.zbd`, with
/// the zone-sky containers stored as its children — the shape the mechanism
/// explains, since the apply routine drags that whole subtree to the camera.
///
/// And the measured unknown stays one: `zone2_cloud_floor` and
/// `moon_reflection` are bound names in the scripts but are **not** stored
/// node names in any of these world archives, so where the original's
/// runtime name registry resolves them is unrecorded — asserted as absent
/// rather than guessed at.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t541_retail_the_bound_horizon_is_an_object3d_under_world1() {
    use cs_formats::gamez::read_gamez_nodes;

    let game_dir = retail_dir();
    let discovery = cs_assets::install::discover(&game_dir).expect("production discovery reads it");
    let manifest_sha = |key: &str| {
        discovery
            .manifest
            .files
            .iter()
            .find(|row| row.relative_spelling.logical_key() == key)
            .map(|row| row.sha256)
            .unwrap_or_else(|| panic!("{key} is inventoried"))
    };

    let mut checked = 0_usize;
    for group in ["C1", "C1B", "C1C", "C2", "C2B", "C3", "C4", "C5"] {
        let rel = format!("ZBD/{group}/gamez.zbd");
        let label = format!("zbd/{}/gamez.zbd", group.to_ascii_lowercase());
        let bytes =
            std::fs::read(game_dir.join(&rel)).expect("the world group's archive must be there");
        assert_eq!(
            cs_assets::install::sha256(&bytes),
            manifest_sha(&label),
            "the node array this test reads is the manifest's own: {rel}"
        );
        let nodes = read_gamez_nodes(&mut ParseContext::with_defaults(&rel), &bytes)
            .unwrap_or_else(|error| panic!("{rel}: the node array reads: {error}"));

        let horizon = nodes
            .nodes
            .iter()
            .find(|node| node.name.eq_ignore_ascii_case("horizon"))
            .unwrap_or_else(|| panic!("{rel}: a `horizon` node is stored"));
        assert!(
            horizon.object3d().is_some(),
            "{rel}: `horizon` is a positioned object3d record, kind {}",
            horizon.kind.label()
        );
        let parent = horizon
            .parent
            .and_then(|slot| nodes.get(slot))
            .unwrap_or_else(|| panic!("{rel}: `horizon`'s stored parent slot resolves"));
        assert_eq!(
            parent.name, "world1",
            "{rel}: the sky container hangs directly off the world root"
        );
        let zones: Vec<&str> = horizon
            .children
            .iter()
            .filter_map(|slot| nodes.get(*slot))
            .map(|child| child.name.as_str())
            .filter(|name| name.starts_with("zone"))
            .collect();
        assert!(
            !zones.is_empty(),
            "{rel}: the zone-sky containers are `horizon`'s children: {zones:?}"
        );

        // The two lateral-anchor names the scripts bind are not stored node
        // names in this archive — where the runtime registry resolves them
        // is the recorded unknown, pinned here rather than guessed.
        for name in ["zone2_cloud_floor", "moon_reflection"] {
            assert!(
                !nodes
                    .nodes
                    .iter()
                    .any(|node| node.name.eq_ignore_ascii_case(name)),
                "{rel}: {name} is not a stored world-archive node (its \
                 runtime origin is unrecorded)"
            );
        }
        checked += 1;
    }
    assert_eq!(checked, 8, "every world group's archive was read");
}

// ------------------------------------------------- the measured image -----

/// The mechanism's spans cover the measured bytes: the stored command names,
/// the two field stores the handlers make (`+0x8` and `+0xc`), and the apply
/// routine's own call list — the translate helper twice, the component read
/// once and the scene-graph update twice — decoded from the span itself.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_t541_image_the_mechanism_spans_cover_the_measured_instructions() {
    use cs_app::camera::horizon::{
        HORIZON_APPLY_LENGTH, HORIZON_APPLY_OFFSET, HORIZON_BIND_LENGTH, HORIZON_BIND_OFFSET,
        HORIZON_COMMAND_LENGTH, HORIZON_COMMAND_OFFSET, HORIZON_IMAGE, HORIZON_XZ_BIND_LENGTH,
        HORIZON_XZ_BIND_OFFSET, HORIZON_XZ_COMMAND_LENGTH, HORIZON_XZ_COMMAND_OFFSET,
    };

    let image = cs_content::coordinates::load_engine_image()
        .expect("CS_ENGINE_IMAGE names the owner-supplied decrypted executable");
    assert_eq!(
        image.digest,
        cs_content::coordinates::original_image_digest(),
        "the loader's own check already binds the spans to the measured bytes"
    );

    let mechanism = mechanism_source(&image).expect("the measured image yields its spans");
    for (name, span) in [
        ("command", &mechanism.command),
        ("command_xz", &mechanism.command_xz),
        ("bind", &mechanism.bind),
        ("bind_xz", &mechanism.bind_xz),
        ("apply", &mechanism.apply),
    ] {
        assert_eq!(
            span.install_sha256(),
            image.digest,
            "{name}: the span names the measured image"
        );
        assert_eq!(span.container_path(), HORIZON_IMAGE, "{name}");
        assert_eq!(span.member_key(), None, "{name}: the image is the source");
    }
    assert_eq!(
        (mechanism.command.offset(), mechanism.command.length()),
        (HORIZON_COMMAND_OFFSET, HORIZON_COMMAND_LENGTH)
    );
    assert_eq!(
        (mechanism.command_xz.offset(), mechanism.command_xz.length()),
        (HORIZON_XZ_COMMAND_OFFSET, HORIZON_XZ_COMMAND_LENGTH)
    );
    assert_eq!(
        (mechanism.bind.offset(), mechanism.bind.length()),
        (HORIZON_BIND_OFFSET, HORIZON_BIND_LENGTH)
    );
    assert_eq!(
        (mechanism.bind_xz.offset(), mechanism.bind_xz.length()),
        (HORIZON_XZ_BIND_OFFSET, HORIZON_XZ_BIND_LENGTH)
    );
    assert_eq!(
        (mechanism.apply.offset(), mechanism.apply.length()),
        (HORIZON_APPLY_OFFSET, HORIZON_APPLY_LENGTH)
    );

    // The stored command names, at the spans the record names: the dispatch
    // chain matches `CameraSetHorizonXZ` first because `CameraSetHorizon` is
    // a 16-byte prefix of it.
    let bytes = |span: &SourceSpan| {
        &image.bytes[span.offset() as usize..(span.offset() + span.length()) as usize]
    };
    assert_eq!(
        bytes(&mechanism.command),
        b"CameraSetHorizon\0",
        "the dispatch's matched name, stored"
    );
    assert_eq!(
        bytes(&mechanism.command_xz),
        b"CameraSetHorizonXZ\0",
        "the longer name, checked first"
    );

    // The handlers' field stores, at the recorded instructions:
    // `mov [ecx+8], eax` at VA 0x4d2f2e and `mov [ecx+0xc], eax` at 0x4d2fae
    // — index 0x6e into each span — which is the camera data slot the apply
    // routine later reads back.
    assert_eq!(
        &bytes(&mechanism.bind)[0x6e..0x71],
        &[0x89, 0x41, 0x08],
        "the CameraSetHorizon handler stores the resolved node at +0x8"
    );
    assert_eq!(
        &bytes(&mechanism.bind_xz)[0x6e..0x71],
        &[0x89, 0x41, 0x0c],
        "the CameraSetHorizonXZ handler stores the resolved node at +0xc"
    );

    // The apply routine reads the bound nodes at +0x8/+0xc — `8b 48 08` at
    // index 5 and `8b 48 0c` at index 0x30 of the span — and every
    // `e8 rel32` call inside it must be one of the three routines the
    // analysis names: the node's own component read (0x4d1cc0), the
    // translate write (0x4d1d50) and the scene-graph update (0x4d0210).
    let apply = bytes(&mechanism.apply);
    assert_eq!(&apply[5..8], &[0x8b, 0x48, 0x08], "reads the +0x8 node");
    assert_eq!(
        &apply[0x30..0x33],
        &[0x8b, 0x48, 0x0c],
        "reads the +0xc node"
    );
    let mut targets: Vec<u32> = (0..apply.len().saturating_sub(5))
        .filter(|&index| apply[index] == 0xe8)
        .map(|index| {
            let insn_va = 0x40_0000_u64 + mechanism.apply.offset() + index as u64;
            let relative =
                i32::from_le_bytes(apply[index + 1..index + 5].try_into().expect("four bytes"));
            insn_va.wrapping_add(5).wrapping_add_signed(relative as i64) as u32
        })
        .collect();
    targets.sort_unstable();
    assert_eq!(
        targets,
        vec![0x4d0210, 0x4d0210, 0x4d1cc0, 0x4d1d50, 0x4d1d50],
        "the apply routine's own calls: two translate writes, the node's \
         component read and the update pass that runs when a binding applied"
    );

    write_mechanism_census(&mechanism, &image, &targets);
}

// ------------------------------------------------- the evidence artifacts ---

/// The private evidence directory the artifacts share with the harness, or
/// `None` outside an evidence run — the test asserts the same measurements
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

/// Writes the measured binding census under `CS_EVIDENCE_DIR` when the
/// evidence run sets it, so the acceptance report carries this run's own
/// rows instead of only the log lines they are asserted on.
///
/// What it holds is **derived** data: the script each occurrence was read
/// from, its stored offset, the bound node name and the rule its command
/// installs. No original display text, no stored geometry and no decompiled
/// code. The file carries the candidate tree it was produced from so a
/// census left over from an earlier commit is refused rather than reused.
fn write_binding_census(
    census: &ViewControlCensus,
    install_sha256: ContentHash,
    content_sha256: ContentHash,
) {
    let Some(directory) = evidence_dir() else {
        return;
    };
    let rows: Vec<String> = census
        .all_occurrences()
        .filter(|occurrence| HorizonAnchorRule::for_command(occurrence.command()).is_some())
        .map(|occurrence| {
            let rule = HorizonAnchorRule::for_command(occurrence.command())
                .expect("the filter keeps only measured commands");
            format!(
                "{{\"script\":{},\"offset\":{},\"node\":{},\"rule\":{}}}",
                quote(occurrence.script()),
                occurrence.offset(),
                quote(
                    occurrence
                        .arguments()
                        .first()
                        .expect("a measured occurrence carries its node argument")
                ),
                quote(rule.label()),
            )
        })
        .collect();
    let document = format!(
        "{{\"schema\":\"cs-t541-horizon-bindings/1\",\"candidate_tree\":{},\
         \"install_sha256\":{},\"content_sha256\":{},\"container\":{},\
         \"horizon\":{},\"horizon_xz\":{},\"bindings\":[{}]}}\n",
        quote(&git(&["rev-parse", "HEAD^{tree}"])),
        quote(&install_sha256.to_hex()),
        quote(&content_sha256.to_hex()),
        quote(census.container()),
        census
            .row("CameraSetHorizon")
            .map(|row| row.count())
            .unwrap_or(0),
        census
            .row("CameraSetHorizonXZ")
            .map(|row| row.count())
            .unwrap_or(0),
        rows.join(","),
    );
    let path = directory.join("horizon-bindings.json");
    std::fs::write(&path, &document)
        .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
    println!("wrote {}", path.display());
}

/// Writes the mechanism's measured image record under `CS_EVIDENCE_DIR` when
/// the evidence run sets it: the image's own digest, the five spans the
/// production record names and the apply routine's decoded call targets.
///
/// Everything in it is a number or a digest — the instruction bytes stay in
/// the owner-supplied image, never in Git.
fn write_mechanism_census(
    mechanism: &HorizonMechanism,
    image: &EngineImage,
    apply_call_targets: &[u32],
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
    let targets: Vec<String> = apply_call_targets.iter().map(u32::to_string).collect();
    let document = format!(
        "{{\"schema\":\"cs-t541-horizon-mechanism/1\",\"candidate_tree\":{},\
         \"image_sha256\":{},{},{},{},{},{},\"apply_call_targets\":[{}]}}\n",
        quote(&git(&["rev-parse", "HEAD^{tree}"])),
        quote(&image.digest.to_hex()),
        span("command", &mechanism.command),
        span("command_xz", &mechanism.command_xz),
        span("bind", &mechanism.bind),
        span("bind_xz", &mechanism.bind_xz),
        span("apply", &mechanism.apply),
        targets.join(","),
    );
    let path = directory.join("horizon-mechanism.json");
    std::fs::write(&path, &document)
        .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
    println!("wrote {}", path.display());
}
