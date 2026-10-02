//! Acceptance scenarios for the scene hierarchy, the LOD rule, the part
//! sockets and the airframe roster audit:
//!
//! * F11-A (AC01): nested transforms and negative scale preserve
//!   visual/collision alignment after canonical conversion, plus the
//!   hierarchy-validation failure cases and the semantic binding records.
//! * F11-B (AC02): the LOD selection rule over converted bands.
//! * F11-C: a part socket carries its role, its one composed pose and the
//!   provenance of the rule that bound it.
//! * F11-D (AC04): the roster audit maps every root, part, mount and cockpit
//!   binding it can reach, or names a typed blocker — over synthetic fixtures
//!   and, in the `#[ignore]`d retail test, over the owner's installation.
//!
//! These tests exercise production code only: `cs_content::scene` over the
//! declared `cs_content::coordinates` adapters and the `cs_types` identity
//! records. Removing or neutering the axis-map conjugation, the composition
//! order, the mirror tracking, the link validation, the LOD band rule or the
//! roster audit makes them fail.
//!
//! Every fixture value is newly authored. The only tests that read original
//! data are `#[ignore = "requires CS_GAME_DIR"]` and fail loudly without
//! `$CS_GAME_DIR`; nothing derived from them beyond counts, offsets and
//! digests is committed.

use std::collections::BTreeMap;

use cs_content::coordinates::SourceAdapter;
use cs_content::scene::{
    AirframeBlocker, AirframeRoster, AnimationBinding, AuditGap, AuthoredTransform, BindingMap,
    CollisionRole, ContainerBlocker, ContainerOutcome, ForcedMissionAssignment, GameZSceneError,
    LodChoice, LodCoverage, LodInfo, LodSelectError, MeshBinding, MeshSlot, NodeKind, ParsedNode,
    ParsedNodeKind, PartRole, RosterAvailability, RosterEntry, RosterError, SceneContainerRef,
    SceneError, SceneGraph, SceneNodeId, SceneRootRef, SemanticBinding, parsed_nodes_from_gamez,
    scene_graph_from_gamez, select_lod_variant,
};
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::space::Meters;

const EPSILON: f64 = 1e-9;

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("test id is valid")
}

fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("test claim id is valid")
}

fn designed(id: &str) -> Provenance {
    Provenance::designed(claim(id))
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, designed("f11a.test.binding")))
}

/// The declared synthetic left-handed-centimeters-degrees adapter from
/// F16-A's registry: canonical X ← source +Y, canonical Y ← source +Z,
/// canonical Z ← source −X, 0.01 m per unit, scalar angles in degrees.
fn fixture_adapter() -> SourceAdapter {
    SourceAdapter::declared()
        .into_iter()
        .find(|adapter| adapter.source().label() == "fixture.left-handed-z-up-centimeters-degrees")
        .expect("the F16-A registry declares the left-handed centimeters fixture")
}

/// The F16-A `canonical` adapter: identity axis map, radians, one unit per
/// metre.
///
/// The node-array tests need it, and not only for tidiness. A CS node record
/// stores its euler triple in **radians** — the pinned reference composes the
/// stored matrix with the raw numbers and asserts each component lies inside
/// `[-π, π]`, and the measured corpus tops out at exactly π — while the
/// F11-A conversion routes the triple through the declared adapter's angle unit.
/// With a degrees-declared adapter a stored π/2 would be read as π/2 *degrees*,
/// so this suite's composition expectations would be about the adapter rather
/// than about the node array. `canonical` makes the conversion the identity and
/// leaves the arithmetic the test is actually about.
fn radian_adapter() -> SourceAdapter {
    SourceAdapter::declared()
        .into_iter()
        .find(|adapter| adapter.source().label() == "canonical")
        .expect("the F16-A registry declares the canonical source")
}

fn close(actual: [f64; 3], expected: [f64; 3], what: &str) {
    for axis in 0..3 {
        assert!(
            (actual[axis] - expected[axis]).abs() <= EPSILON,
            "{what}: axis {axis}: {} != {}",
            actual[axis],
            expected[axis]
        );
    }
}

fn close3(actual: [[f64; 3]; 3], expected: [[f64; 3]; 3], what: &str) {
    for row in 0..3 {
        close(actual[row], expected[row], &format!("{what} row {row}"));
    }
}

/// AC01's fixture, in the declared fixture convention (source units are
/// centimeters, rotations are degrees): `main` → `wing` → `gun` → `tip`,
/// where `wing` rotates 90° about source Z and sits 200 cm out, and `gun`
/// mirrors across source X (`scale [-1, 1, 1]`).
///
/// Hand-computed canonical results: `wing` local linear is a +90° rotation
/// about canonical Y, translation [0, 0, −2] m; `gun` local mirrors
/// canonical Z; `gun`/`tip` world is `[[0,0,−1],[0,1,0],[−1,0,0]]` with
/// translation [0, 0, −2.5] m and determinant −1.
fn nested_mirror_fixture() -> Vec<ParsedNode> {
    let mut main = ParsedNode::new(0, "main", ParsedNodeKind::World);
    main.children = vec![1];
    let mut wing = ParsedNode::new(1, "wing", ParsedNodeKind::Object3d);
    wing.parent = Some(0);
    wing.children = vec![2];
    wing.transform = AuthoredTransform {
        rotation: [0.0, 0.0, 90.0],
        ..AuthoredTransform::IDENTITY
    };
    wing.transform.translation = [200.0, 0.0, 0.0];
    let mut gun = ParsedNode::new(2, "gun", ParsedNodeKind::Object3d);
    gun.parent = Some(1);
    gun.children = vec![3];
    gun.transform = AuthoredTransform {
        scale: [-1.0, 1.0, 1.0],
        translation: [0.0, 50.0, 0.0],
        ..AuthoredTransform::IDENTITY
    };
    let mut tip = ParsedNode::new(3, "tip", ParsedNodeKind::Object3d);
    tip.parent = Some(2);
    tip.mesh = Some(MeshBinding {
        index: 7,
        mesh: known(cid(ContentKind::Mesh, "fix_planes.7")),
    });
    vec![main, wing, gun, tip]
}

/// AC01 minimum scenario: a rotation, a translation and a negative scale in
/// a nested hierarchy produce the hand-computed canonical transforms, the
/// mirror flag propagates to descendants, and the render and collision
/// paths read the same composed transform.
#[test]
fn accept_f11_a_nested_transforms_and_negative_scale_preserve_alignment() {
    let container = cid(ContentKind::InstallFile, "fix_planes");
    let scene = SceneGraph::build(
        &container,
        &nested_mirror_fixture(),
        &fixture_adapter(),
        &BindingMap::default(),
    )
    .expect("the fixture hierarchy converts");

    // Stable semantic ids — the name-path, never the array slot.
    let tip_id =
        SceneNodeId::from_content_id(cid(ContentKind::SceneNode, "fix_planes.main.wing.gun.tip"))
            .expect("scene node id");
    let tip = scene.node(&tip_id).expect("tip node exists");
    let main = scene.root("main").expect("the named root resolves");
    let wing = scene.node(&main.children()[0]).expect("child id resolves");
    let gun = scene.node(&wing.children()[0]).expect("child id resolves");
    assert_eq!(scene.node(&gun.children()[0]).expect("child"), tip);

    // Wing local: +90° about canonical Y, 200 cm → [0,0,-2] m.
    close3(
        wing.local_transform().linear(),
        [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]],
        "wing local linear",
    );
    close(
        wing.local_transform().translation(),
        [0.0, 0.0, -2.0],
        "wing local translation",
    );
    assert!(!wing.mirrored(), "a pure rotation does not mirror");

    // Gun local: the authored [-1,1,1] scale mirrors canonical Z after the
    // axis-map conjugation; 50 cm along source Y → +0.5 m canonical X.
    close3(
        gun.local_transform().linear(),
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]],
        "gun local linear",
    );
    close(
        gun.local_transform().translation(),
        [0.5, 0.0, 0.0],
        "gun local translation",
    );

    // Composed world of gun and (identically) tip — the mirror survives.
    let expected_linear = [[0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]];
    let expected_translation = [0.0, 0.0, -2.5];
    close3(
        gun.world_transform().linear(),
        expected_linear,
        "gun world linear",
    );
    close(
        gun.world_transform().translation(),
        expected_translation,
        "gun world translation",
    );
    close3(
        tip.world_transform().linear(),
        expected_linear,
        "tip world linear",
    );
    close(
        tip.world_transform().translation(),
        expected_translation,
        "tip world translation",
    );
    assert!(
        gun.mirrored() && tip.mirrored(),
        "negative scale mirrors the subtree"
    );
    assert!(gun.world_transform().determinant() < 0.0);

    // The alignment claim: render and collision read the same transform, so
    // a mirrored mesh and its collider cannot diverge after conversion.
    for node in scene.nodes() {
        assert_eq!(
            node.visual_transform(),
            node.collision_transform(),
            "node {} must share one transform between visual and collision",
            node.id()
        );
    }
    close(
        tip.world_transform().apply([1.0, 0.0, 0.0]),
        [0.0, 0.0, -3.5],
        "tip world maps its +X point",
    );
    close(
        tip.collision_transform().apply([1.0, 0.0, 0.0]),
        [0.0, 0.0, -3.5],
        "the collision path sees the same point",
    );

    // The authored transform is preserved next to the canonical one, and the
    // mesh association rides along.
    assert_eq!(gun.authored().scale, [-1.0, 1.0, 1.0]);
    assert_eq!(tip.mesh().expect("tip has a mesh").index, 7);
    assert_eq!(
        tip.mesh()
            .expect("tip mesh")
            .mesh
            .provenance()
            .unwrap()
            .claim_id,
        claim("f11a.test.binding")
    );
}

/// A stored 3×3 wins over the euler triple, LOD variants survive conversion
/// with their ranges in meters, bindings attach by authored name-path and
/// unmatched rules are reported.
#[test]
fn accept_f11_a_lod_variants_and_bindings_survive_conversion() {
    let container = cid(ContentKind::InstallFile, "fix_planes");

    let mut main = ParsedNode::new(0, "main", ParsedNodeKind::World);
    main.children = vec![1, 2, 3];
    let mut lod0 = ParsedNode::new(
        1,
        "wing_lod0",
        ParsedNodeKind::Lod {
            level: false,
            range_min: 0.0,
            range_max: 50_000.0,
        },
    );
    lod0.parent = Some(0);
    let mut lod1 = ParsedNode::new(
        2,
        "wing_lod1",
        ParsedNodeKind::Lod {
            level: true,
            range_min: 50_000.0,
            range_max: 200_000.0,
        },
    );
    lod1.parent = Some(0);
    let mut gun = ParsedNode::new(3, "gun", ParsedNodeKind::Object3d);
    gun.parent = Some(0);
    // The stored matrix overrides a contradicting euler triple.
    gun.transform = AuthoredTransform {
        rotation: [0.0, 0.0, 90.0],
        matrix: Some([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]),
        ..AuthoredTransform::IDENTITY
    };
    let mut other = ParsedNode::new(4, "other", ParsedNodeKind::World);
    other.children = vec![5];
    let mut cargo = ParsedNode::new(5, "cargo", ParsedNodeKind::Object3d);
    cargo.parent = Some(4);

    let bindings = BindingMap::new(vec![
        SemanticBinding {
            path: "main.gun".to_owned(),
            role: known(PartRole::Gun),
            collision: known(CollisionRole::Collider),
            animation: vec![AnimationBinding {
                channel: known(cid(ContentKind::AnimationTrack, "recoil")),
            }],
            provenance: designed("f11a.test.gun-rule"),
        },
        SemanticBinding {
            path: "main.absent".to_owned(),
            role: known(PartRole::DamageZone),
            collision: known(CollisionRole::None),
            animation: Vec::new(),
            provenance: designed("f11a.test.absent-rule"),
        },
    ])
    .expect("distinct binding paths");

    let scene = SceneGraph::build(
        &container,
        &[main, lod0, lod1, gun, other, cargo],
        &fixture_adapter(),
        &bindings,
    )
    .expect("the LOD/binding fixture converts");

    // Every LOD variant is preserved with its converted range — nothing is
    // flattened.
    let main = scene.root("main").expect("root");
    let lod0 = scene.node(&main.children()[0]).expect("lod0 node");
    let lod1 = scene.node(&main.children()[1]).expect("lod1 node");
    let lod0 = match lod0.kind() {
        NodeKind::Lod(info) => info,
        other => panic!("lod0 must be an LOD node, got {other:?}"),
    };
    assert!(!lod0.level);
    assert_eq!(lod0.range_min.0, 0.0);
    assert!(
        (lod0.range_max.0 - 500.0).abs() <= EPSILON,
        "50 000 cm must convert to 500 m, got {}",
        lod0.range_max.0
    );
    match lod1.kind() {
        NodeKind::Lod(info) => {
            assert!(info.level);
            assert!((info.range_min.0 - 500.0).abs() <= EPSILON);
            assert!((info.range_max.0 - 2000.0).abs() <= EPSILON);
        }
        other => panic!("lod1 must be an LOD node, got {other:?}"),
    }

    // The stored matrix overrode the euler triple: `gun`'s local linear is
    // the conjugated identity, not a rotation.
    let gun = scene.node(&main.children()[2]).expect("gun node");
    close3(
        gun.local_transform().linear(),
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        "stored matrix wins over the euler triple",
    );

    // The binding landed on the authored path; its role, collision role and
    // animation channel are known with the rule's provenance.
    let binding = gun.binding().expect("the rule bound main.gun");
    assert_eq!(binding.role, known(PartRole::Gun));
    assert_eq!(binding.collision, known(CollisionRole::Collider));
    assert_eq!(binding.animation.len(), 1);
    assert_eq!(binding.provenance, designed("f11a.test.gun-rule"));

    // A rule that names no node is reported, not dropped.
    assert_eq!(scene.unmatched_bindings(), &["main.absent".to_owned()]);

    // Visibility is honestly unknown: the CS flag bits are unmeasured, so
    // the field exists but carries an explicit unknown, not a guess.
    for node in scene.nodes() {
        match node.visibility() {
            Resolved::Unknown { claim_id, reason } => {
                assert_eq!(claim_id, &claim("f11a.node-flags-unmeasured"));
                assert!(!reason.is_empty());
            }
            Resolved::Known(known) => panic!(
                "node {} visibility {known:?} was invented — flag bits are unmeasured",
                node.id()
            ),
        }
    }

    // Two roots make `single_root` ambiguous; a named lookup still works and
    // a missing name is an error, not a fallback.
    assert_eq!(
        scene.single_root().map(|_| ()),
        Err(SceneError::AmbiguousRoots { roots: 2 })
    );
    assert_eq!(
        scene.root("nope").map(|_| ()),
        Err(SceneError::UnknownRoot {
            name: "nope".to_owned()
        })
    );
    assert_eq!(scene.root("other").expect("other root").name(), "other");
    let cargo = scene
        .node(&scene.root("other").expect("root").children()[0].clone())
        .expect("cargo node");
    assert_eq!(cargo.id().key(), "fix_planes.other.cargo");
}

/// Every structural rejection of `SceneGraph::build`: the harness fails
/// visibly, never silently repairs a hierarchy.
#[test]
fn accept_f11_a_rejects_cycles_dangling_parents_and_ambiguous_roots() {
    let container = cid(ContentKind::InstallFile, "fix_planes");
    let adapter = fixture_adapter();
    let no_bindings = BindingMap::default();
    let build = |nodes: &[ParsedNode]| SceneGraph::build(&container, nodes, &adapter, &no_bindings);

    // Empty input has no roots at all.
    assert_eq!(build(&[]).map(|_| ()), Err(SceneError::EmptyScene));

    // A parent slot with no record is dangling.
    let mut dangling = ParsedNode::new(0, "orphan", ParsedNodeKind::Object3d);
    dangling.parent = Some(9);
    assert_eq!(
        build(&[dangling]).map(|_| ()),
        Err(SceneError::DanglingParent { node: 0, parent: 9 })
    );

    // A child slot with no record is dangling.
    let mut root = ParsedNode::new(0, "main", ParsedNodeKind::World);
    root.children = vec![9];
    assert_eq!(
        build(&[root]).map(|_| ()),
        Err(SceneError::DanglingChild { node: 0, child: 9 })
    );

    // The link must agree in both directions: a parent slot the parent does
    // not list back is inconsistent, and a listed child that names another
    // parent is too.
    let mut root = ParsedNode::new(0, "main", ParsedNodeKind::World);
    let mut child = ParsedNode::new(1, "wing", ParsedNodeKind::Object3d);
    child.parent = Some(0);
    assert_eq!(
        build(&[root.clone(), child.clone()]).map(|_| ()),
        Err(SceneError::InconsistentParentage { node: 1, parent: 0 })
    );
    root.children = vec![1];
    child.parent = None;
    assert_eq!(
        build(&[root, child]).map(|_| ()),
        Err(SceneError::InconsistentParentage { node: 1, parent: 0 })
    );

    // Two mutually-parented nodes with a healthy root elsewhere form a
    // detached ownership cycle; a wholly parentless-less array has no root.
    let root = ParsedNode::new(0, "main", ParsedNodeKind::World);
    let mut a = ParsedNode::new(1, "a", ParsedNodeKind::Object3d);
    a.parent = Some(2);
    a.children = vec![2];
    let mut b = ParsedNode::new(2, "b", ParsedNodeKind::Object3d);
    b.parent = Some(1);
    b.children = vec![1];
    assert_eq!(
        build(&[a.clone(), b.clone()]).map(|_| ()),
        Err(SceneError::NoRoots)
    );
    let cycled = build(&[root, a, b]).map(|_| ());
    assert!(
        matches!(cycled, Err(SceneError::Cycle { node: 1 | 2 })),
        "the detached cycle must be reported, got {cycled:?}"
    );

    // Duplicate stored slots are refused before any link is trusted.
    let dup_a = ParsedNode::new(0, "main", ParsedNodeKind::World);
    let dup_b = ParsedNode::new(0, "other", ParsedNodeKind::World);
    assert_eq!(
        build(&[dup_a, dup_b]).map(|_| ()),
        Err(SceneError::DuplicateIndex { index: 0 })
    );

    // Two roots carrying the same authored name are ambiguous — the
    // collision is refused rather than disambiguated by position.
    let first = ParsedNode::new(0, "main", ParsedNodeKind::World);
    let second = ParsedNode::new(1, "main", ParsedNodeKind::World);
    let collided = build(&[first, second]).map(|_| ());
    assert!(
        matches!(collided, Err(SceneError::DuplicateNodeId { .. })),
        "same-named roots must collide, got {collided:?}"
    );

    // An authored name that cannot form a content key is refused by name.
    let bad = ParsedNode::new(0, "bad name", ParsedNodeKind::World);
    assert_eq!(
        build(&[bad]).map(|_| ()),
        Err(SceneError::NodeId {
            node: 0,
            source: cs_types::content::ContentIdError::BadKeyCharacter { ch: ' ' }
        })
    );
}

/// Field-level refusals: non-finite transforms, bad LOD ranges, wrong-kind
/// mesh and animation ids, and duplicate binding rules.
#[test]
fn accept_f11_a_rejects_bad_transforms_and_wrong_binding_kinds() {
    let container = cid(ContentKind::InstallFile, "fix_planes");
    let adapter = fixture_adapter();
    let no_bindings = BindingMap::default();

    // A non-finite authored component is refused at the boundary.
    let mut bad = ParsedNode::new(0, "main", ParsedNodeKind::Object3d);
    bad.transform.translation = [f32::NAN, 0.0, 0.0];
    assert_eq!(
        SceneGraph::build(&container, &[bad], &adapter, &no_bindings).map(|_| ()),
        Err(SceneError::Transform {
            node: 0,
            source: cs_types::space::SpaceError::NonFinite {
                field: "translation[0]"
            }
        })
    );

    // Reversed, negative and non-finite LOD ranges are all refused.
    for (range_min, range_max) in [
        (200.0, 100.0),
        (-1.0, 100.0),
        (f32::NAN, 100.0),
        (0.0, f32::INFINITY),
    ] {
        let lod = ParsedNode::new(
            0,
            "main",
            ParsedNodeKind::Lod {
                level: false,
                range_min,
                range_max,
            },
        );
        let outcome = SceneGraph::build(&container, &[lod], &adapter, &no_bindings).map(|_| ());
        match outcome {
            Err(SceneError::LodRange {
                node,
                range_min: min,
                range_max: max,
            }) => {
                assert_eq!(node, 0);
                assert!(
                    (min == range_min || (min.is_nan() && range_min.is_nan()))
                        && (max == range_max || (max.is_nan() && range_max.is_nan())),
                    "range {range_min}..{range_max} must be refused with its values, got {min}..{max}"
                );
            }
            other => panic!("range {range_min}..{range_max} must be refused, got {other:?}"),
        }
    }

    // A mesh binding may only resolve to a `mesh` element.
    let mut meshy = ParsedNode::new(0, "main", ParsedNodeKind::Object3d);
    meshy.mesh = Some(MeshBinding {
        index: 0,
        mesh: known(cid(ContentKind::Gun, "fix_planes.0")),
    });
    assert_eq!(
        SceneGraph::build(&container, &[meshy], &adapter, &no_bindings).map(|_| ()),
        Err(SceneError::MeshKind {
            node: 0,
            kind: ContentKind::Gun
        })
    );

    // An animation channel may only resolve to an `animation_track`.
    let root = ParsedNode::new(0, "main", ParsedNodeKind::World);
    let wrong_channel = BindingMap::new(vec![SemanticBinding {
        path: "main".to_owned(),
        role: known(PartRole::Engine),
        collision: known(CollisionRole::None),
        animation: vec![AnimationBinding {
            channel: known(cid(ContentKind::Mesh, "fix_planes.0")),
        }],
        provenance: designed("f11a.test.wrong-channel"),
    }])
    .expect("one rule");
    assert_eq!(
        SceneGraph::build(&container, &[root], &adapter, &wrong_channel).map(|_| ()),
        Err(SceneError::AnimationChannelKind {
            node: 0,
            kind: ContentKind::Mesh
        })
    );

    // Two rules naming one path are an ambiguous mapping, refused at
    // construction.
    let rule = |role: PartRole| SemanticBinding {
        path: "main.gun".to_owned(),
        role: known(role),
        collision: known(CollisionRole::None),
        animation: Vec::new(),
        provenance: designed("f11a.test.rule"),
    };
    assert_eq!(
        BindingMap::new(vec![rule(PartRole::Gun), rule(PartRole::DamageZone)]).map(|_| ()),
        Err(SceneError::DuplicateBindingRule {
            path: "main.gun".to_owned()
        })
    );

    // Id-kind and root-reference validation on the contract records.
    assert_eq!(
        SceneNodeId::from_content_id(cid(ContentKind::Mesh, "fix_planes.0")).map(|_| ()),
        Err(SceneError::NodeKind {
            kind: ContentKind::Mesh
        })
    );
    let nested = SceneNodeId::from_content_id(cid(ContentKind::SceneNode, "fix_planes.main.wing"))
        .expect("nested id");
    assert_eq!(
        SceneRootRef::new(container.clone(), nested).map(|_| ()),
        Err(SceneError::NotARootNode {
            root: "fix_planes.main.wing".to_owned()
        })
    );
    let foreign = SceneNodeId::from_content_id(cid(ContentKind::SceneNode, "gamez.main"))
        .expect("foreign id");
    assert_eq!(
        SceneRootRef::new(container, foreign).map(|_| ()),
        Err(SceneError::RootOutsideContainer {
            container: "fix_planes".to_owned(),
            root: "gamez.main".to_owned()
        })
    );
}

// ------------------------------------------------------------ F11-B (LOD) ---

/// The `Lod` sibling group of one container, converted by the production
/// build so the rule is exercised over ranges that really went through the
/// axis map: `0..500 m`, `500..2000 m` and `3000..4000 m` (source centimetres).
fn lod_bands() -> Vec<cs_content::scene::LodInfo> {
    let container = cid(ContentKind::InstallFile, "fix_planes");

    let mut main = ParsedNode::new(0, "main", ParsedNodeKind::World);
    main.children = vec![1, 2, 3];
    let mut near = ParsedNode::new(
        1,
        "band_near",
        ParsedNodeKind::Lod {
            level: false,
            range_min: 0.0,
            range_max: 50_000.0,
        },
    );
    near.parent = Some(0);
    let mut wide = ParsedNode::new(
        2,
        "band_wide",
        ParsedNodeKind::Lod {
            level: true,
            range_min: 50_000.0,
            range_max: 200_000.0,
        },
    );
    wide.parent = Some(0);
    let mut far = ParsedNode::new(
        3,
        "band_far",
        ParsedNodeKind::Lod {
            level: true,
            range_min: 300_000.0,
            range_max: 400_000.0,
        },
    );
    far.parent = Some(0);

    let scene = SceneGraph::build(
        &container,
        &[main, near, wide, far],
        &fixture_adapter(),
        &BindingMap::default(),
    )
    .expect("the LOD group converts");
    scene
        .nodes()
        .iter()
        .filter_map(|node| node.lod().copied())
        .collect()
}

/// AC02's selection rule: the distance falls in exactly one band, on a shared
/// band edge, or in an authored gap — and every answer says which it was, in
/// stored order, over ranges the real conversion produced.
#[test]
fn accept_f11_b_lod_selection_rule_reports_coverage_gaps_and_overlaps() {
    let bands = lod_bands();
    assert_eq!(bands.len(), 3);
    assert_eq!(
        bands
            .iter()
            .map(|band| (band.range_min.0, band.range_max.0))
            .collect::<Vec<_>>(),
        vec![(0.0, 500.0), (500.0, 2000.0), (3000.0, 4000.0)],
        "the authored centimetre ranges arrive as metres"
    );

    // Inside a band: exactly one variant covers the distance.
    assert_eq!(
        select_lod_variant(&bands, Meters(250.0)),
        Ok(LodChoice {
            index: 0,
            coverage: LodCoverage::Covered
        })
    );
    assert_eq!(
        select_lod_variant(&bands, Meters(750.0)),
        Ok(LodChoice {
            index: 1,
            coverage: LodCoverage::Covered
        })
    );

    // Adjacent bands share their edge: the tightest band wins, and the tie
    // is reported instead of being passed off as a plain coverage.
    assert_eq!(
        select_lod_variant(&bands, Meters(500.0)),
        Ok(LodChoice {
            index: 0,
            coverage: LodCoverage::Overlap
        })
    );

    // A gap is reported and the nearest band keeps the part on screen: above
    // the second band, and between the second and the third where two bands
    // are equally near (ties go to the lower near bound, then stored order).
    assert_eq!(
        select_lod_variant(&bands, Meters(2500.0)),
        Ok(LodChoice {
            index: 1,
            coverage: LodCoverage::GapFallback
        })
    );
    assert_eq!(
        select_lod_variant(&bands, Meters(2100.0)),
        Ok(LodChoice {
            index: 1,
            coverage: LodCoverage::GapFallback
        })
    );
    assert_eq!(
        select_lod_variant(&bands, Meters(4500.0)),
        Ok(LodChoice {
            index: 2,
            coverage: LodCoverage::GapFallback
        })
    );

    // A distance below every band still lands on a band, deterministically.
    assert_eq!(
        select_lod_variant(&[bands[1]], Meters(10.0)),
        Ok(LodChoice {
            index: 0,
            coverage: LodCoverage::GapFallback
        })
    );
}

/// The rule refuses input it cannot reason about instead of deciding
/// presentation from it: an empty group, an unusable distance and a
/// non-finite, negative or reversed band.
#[test]
fn accept_f11_b_lod_selection_rule_refuses_unusable_input() {
    let bands = lod_bands();

    assert_eq!(
        select_lod_variant(&[], Meters(10.0)).map(|_| ()),
        Err(LodSelectError::NoVariants)
    );
    for distance in [f64::NAN, f64::INFINITY, -1.0] {
        assert_eq!(
            select_lod_variant(&bands, Meters(distance)).map(|_| ()),
            Err(LodSelectError::Distance),
            "distance {distance} must not select anything"
        );
    }

    let usable = LodInfo {
        level: false,
        range_min: Meters(0.0),
        range_max: Meters(100.0),
    };
    let reversed = LodInfo {
        range_min: Meters(200.0),
        range_max: Meters(100.0),
        ..usable
    };
    assert_eq!(
        select_lod_variant(&[usable, reversed], Meters(10.0)).map(|_| ()),
        Err(LodSelectError::Range { index: 1 }),
        "the offending band is named by its stored position"
    );
    for bad in [
        LodInfo {
            range_min: Meters(f64::NAN),
            ..usable
        },
        LodInfo {
            range_max: Meters(f64::NAN),
            ..usable
        },
        LodInfo {
            range_min: Meters(-1.0),
            ..usable
        },
    ] {
        assert_eq!(
            select_lod_variant(&[bad], Meters(10.0)).map(|_| ()),
            Err(LodSelectError::Range { index: 0 })
        );
    }
}

/// An explicit unknown for a value the evidence could not resolve.
fn unmeasured<T>(id: &str, reason: &str) -> Resolved<T> {
    Resolved::unknown(claim(id), reason).expect("the unknown carries a reason")
}

/// F11-C's socket table over AC01's own mirrored hierarchy: the same
/// `nested_mirror_fixture`, so the gun socket's pose is the *mirrored* world
/// transform the render and collision paths already agree on, plus one
/// socket whose role was never evidenced (`main.wing.gun.tip`) and one rule
/// that names no node at all (`main.absent`).
fn socket_bindings() -> BindingMap {
    BindingMap::new(vec![
        SemanticBinding {
            path: "main.wing".to_owned(),
            role: known(PartRole::ControlSurface),
            collision: known(CollisionRole::None),
            animation: Vec::new(),
            provenance: designed("f11c.test.wing-rule"),
        },
        SemanticBinding {
            animation: vec![AnimationBinding {
                channel: known(cid(ContentKind::AnimationTrack, "recoil")),
            }],
            ..SemanticBinding {
                path: "main.wing.gun".to_owned(),
                role: known(PartRole::Gun),
                collision: known(CollisionRole::Collider),
                animation: Vec::new(),
                provenance: designed("f11c.test.gun-rule"),
            }
        },
        SemanticBinding {
            path: "main.wing.gun.tip".to_owned(),
            role: unmeasured(
                "f11c.test.tip-role-unmeasured",
                "no evidence named the tip's gameplay role",
            ),
            collision: unmeasured(
                "f11c.test.tip-collision-unmeasured",
                "no evidence named the tip's collision role",
            ),
            animation: Vec::new(),
            provenance: designed("f11c.test.tip-rule"),
        },
        SemanticBinding {
            path: "main.absent".to_owned(),
            role: known(PartRole::Cockpit),
            collision: known(CollisionRole::None),
            animation: Vec::new(),
            provenance: designed("f11c.test.absent-rule"),
        },
    ])
    .expect("the socket fixture rules name distinct paths")
}

/// F11-C: the semantic sockets a runtime consumer binds to. One socket per
/// bound node, in stable-id order; the role, collision role, zone, animation
/// channels and provenance come from the rule; the pose is the node's one
/// composed transform (the mirrored one AC01 checks), so a mount point can
/// never drift from collision; a node no rule named is not a socket; a rule
/// whose role is an explicit unknown still yields a socket that is listed as
/// unresolved and is not given a default role; and a rule that names no node
/// is reported rather than dropped.
#[test]
fn accept_f11_c_sockets_carry_roles_poses_and_provenance() {
    let container = cid(ContentKind::InstallFile, "fix_planes");
    let scene = SceneGraph::build(
        &container,
        &nested_mirror_fixture(),
        &fixture_adapter(),
        &socket_bindings(),
    )
    .expect("the socket fixture converts");

    let node = |path: &str| {
        SceneNodeId::from_content_id(cid(ContentKind::SceneNode, path)).expect("scene node id")
    };

    // Every bound node is a socket exactly once, ordered by stable id.
    let sockets: Vec<String> = scene
        .sockets()
        .map(|socket| socket.node().key().to_owned())
        .collect();
    assert_eq!(
        sockets,
        vec![
            "fix_planes.main.wing".to_owned(),
            "fix_planes.main.wing.gun".to_owned(),
            "fix_planes.main.wing.gun.tip".to_owned(),
        ],
        "one socket per bound node, in stable-id order"
    );

    // The gun socket: an evidenced role with its own provenance, the stored
    // zone, the bound animation channel — and the node's one composed pose.
    let gun = scene
        .socket(&node("fix_planes.main.wing.gun"))
        .expect("the rule bound main.wing.gun");
    assert_eq!(gun.known_role(), Some(PartRole::Gun));
    assert_eq!(gun.role(), &known(PartRole::Gun));
    assert_eq!(gun.collision(), &known(CollisionRole::Collider));
    assert_eq!(gun.zone_id(), 255, "the fixture node carries no zone");
    assert_eq!(gun.animation().len(), 1);
    assert_eq!(
        gun.animation()[0].channel,
        known(cid(ContentKind::AnimationTrack, "recoil"))
    );
    assert_eq!(gun.provenance(), &designed("f11c.test.gun-rule"));

    // The pose is the node's single composed transform — the same value the
    // render path draws and collision evaluates, mirror included.
    let gun_node = scene
        .node(&node("fix_planes.main.wing.gun"))
        .expect("the gun node");
    assert_eq!(gun.pose(), gun_node.world_transform());
    assert_eq!(gun.pose(), gun_node.visual_transform());
    assert_eq!(gun.pose(), gun_node.collision_transform());
    close3(
        gun.pose().linear(),
        [[0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]],
        "the socket keeps the mirrored composed linear map",
    );
    close(
        gun.pose().translation(),
        [0.0, 0.0, -2.5],
        "the socket keeps the composed translation",
    );
    assert!(
        gun.pose().mirrored(),
        "a mirrored mount stays mirrored in the socket record"
    );

    // Lookup by role, and by identity.
    assert_eq!(
        scene
            .sockets_of_role(PartRole::ControlSurface)
            .map(|socket| socket.node().key().to_owned())
            .collect::<Vec<_>>(),
        vec!["fix_planes.main.wing".to_owned()]
    );
    assert_eq!(
        scene
            .sockets_of_role(PartRole::Gun)
            .map(|socket| socket.node().key().to_owned())
            .collect::<Vec<_>>(),
        vec!["fix_planes.main.wing.gun".to_owned()]
    );
    assert_eq!(scene.sockets_of_role(PartRole::Cockpit).count(), 0);
    assert!(
        scene.socket(&node("fix_planes.main")).is_none(),
        "a node no rule named is not a socket"
    );

    // The unmeasured role is reported, never defaulted: the socket exists and
    // says what it does not know.
    let unresolved: Vec<String> = scene
        .unresolved_sockets()
        .map(|socket| socket.node().key().to_owned())
        .collect();
    assert_eq!(unresolved, vec!["fix_planes.main.wing.gun.tip".to_owned()]);
    let tip = scene
        .socket(&node("fix_planes.main.wing.gun.tip"))
        .expect("the rule bound main.wing.gun.tip");
    assert_eq!(tip.known_role(), None);
    match tip.role() {
        Resolved::Unknown { claim_id, reason } => {
            assert_eq!(claim_id, &claim("f11c.test.tip-role-unmeasured"));
            assert!(!reason.is_empty());
        }
        Resolved::Known(known) => panic!("the tip's role {known:?} was invented"),
    }
    assert!(
        !scene
            .sockets_of_role(PartRole::DamageZone)
            .any(|socket| socket.node() == tip.node()),
        "an unmeasured role is in no role's list"
    );
    assert_eq!(
        scene
            .node(&node("fix_planes.main.wing.gun.tip"))
            .expect("the tip node")
            .world_transform(),
        tip.pose(),
        "even an unresolved socket keeps the node's pose"
    );

    // A rule that names no node is still reported.
    assert_eq!(scene.unmatched_bindings(), &["main.absent".to_owned()]);
}

// ------------------------------------------------------------- F11-D (AC04) ---

/// Appends a node with the given stored slot and, when it has one, links it
/// into its parent's child list. The stored slot is deliberately not the
/// vector position, so a fixture that skips slots still wires correctly.
fn append(
    nodes: &mut Vec<ParsedNode>,
    slot_of: &mut std::collections::BTreeMap<u32, usize>,
    index: u32,
    name: &str,
    parent: Option<u32>,
) {
    let mut node = ParsedNode::new(index, name, ParsedNodeKind::Object3d);
    node.parent = parent;
    if let Some(parent) = parent {
        let &position = slot_of
            .get(&parent)
            .expect("the parent is already in the fixture");
        nodes[position].children.push(index);
    }
    slot_of.insert(index, nodes.len());
    nodes.push(node);
}

/// The AC04 fixture's airframe container: two roots. `alpha` is the fully
/// evidenced airframe — a cockpit, a control surface, a gun, a rocket mount,
/// an engine, a camera anchor and a damage zone. `beta` is the mission-only
/// shape: a cockpit, a control surface, a gun, and one bound node whose role
/// nobody evidenced.
///
/// Thirteen stored records, so a container that declares thirteen converts
/// cleanly and one that declares another number does not.
fn roster_container_fixture() -> Vec<ParsedNode> {
    let mut nodes: Vec<ParsedNode> = Vec::new();
    let mut slot_of = std::collections::BTreeMap::new();
    let root = ParsedNode::new(0, "alpha", ParsedNodeKind::World);
    slot_of.insert(0, 0);
    nodes.push(root);
    append(&mut nodes, &mut slot_of, 1, "cockpit", Some(0));
    append(&mut nodes, &mut slot_of, 2, "wing_l", Some(0));
    // The wing carries an authored rotation and offset, so everything mounted
    // under it has a local transform that differs from its composed world
    // pose: a socket that copied the local transform would be caught.
    nodes[slot_of[&2]].transform = AuthoredTransform {
        rotation: [0.0, 0.0, 90.0],
        translation: [200.0, 0.0, 0.0],
        ..AuthoredTransform::IDENTITY
    };
    append(&mut nodes, &mut slot_of, 3, "rocket", Some(0));
    append(&mut nodes, &mut slot_of, 4, "engine", Some(0));
    append(&mut nodes, &mut slot_of, 5, "camera", Some(0));
    append(&mut nodes, &mut slot_of, 6, "hull", Some(0));
    append(&mut nodes, &mut slot_of, 7, "gun_l", Some(2));
    nodes[slot_of[&7]].zone_id = 42;
    let beta = ParsedNode::new(10, "beta", ParsedNodeKind::World);
    slot_of.insert(10, nodes.len());
    nodes.push(beta);
    append(&mut nodes, &mut slot_of, 11, "cockpit_b", Some(10));
    append(&mut nodes, &mut slot_of, 12, "wing_b", Some(10));
    nodes[slot_of[&12]].transform = AuthoredTransform {
        rotation: [0.0, 0.0, -90.0],
        translation: [-150.0, 0.0, 0.0],
        ..AuthoredTransform::IDENTITY
    };
    append(&mut nodes, &mut slot_of, 13, "gun_b", Some(12));
    append(&mut nodes, &mut slot_of, 8, "pod_b", Some(13));
    nodes
}

/// The AC04 fixture's binding rules: every role the sheet names, on
/// `alpha` and `beta` in turn, plus `alpha.pod` whose role and collision role
/// are explicit unknowns, plus `alpha.absent`, a rule that matches no node at
/// all and is therefore reported.
fn roster_bindings() -> BindingMap {
    let role = |path: &str, role: PartRole, id: &str| SemanticBinding {
        path: path.to_owned(),
        role: known(role),
        collision: known(CollisionRole::Collider),
        animation: if role == PartRole::Gun {
            vec![AnimationBinding {
                channel: known(cid(ContentKind::AnimationTrack, "recoil")),
            }]
        } else {
            Vec::new()
        },
        provenance: designed(id),
    };
    BindingMap::new(vec![
        role(
            "alpha.cockpit",
            PartRole::Cockpit,
            "f11d.test.alpha-cockpit",
        ),
        role(
            "alpha.wing_l",
            PartRole::ControlSurface,
            "f11d.test.alpha-wing",
        ),
        role("alpha.wing_l.gun_l", PartRole::Gun, "f11d.test.alpha-gun"),
        role(
            "alpha.rocket",
            PartRole::RocketMount,
            "f11d.test.alpha-rocket",
        ),
        role("alpha.engine", PartRole::Engine, "f11d.test.alpha-engine"),
        role(
            "alpha.camera",
            PartRole::CameraAnchor,
            "f11d.test.alpha-camera",
        ),
        role("alpha.hull", PartRole::DamageZone, "f11d.test.alpha-hull"),
        SemanticBinding {
            path: "beta.wing_b.gun_b.pod_b".to_owned(),
            role: unmeasured(
                "f11d.test.pod-role-unmeasured",
                "no evidence named the pod's gameplay role",
            ),
            collision: unmeasured(
                "f11d.test.pod-collision-unmeasured",
                "no evidence named the pod's collision role",
            ),
            animation: Vec::new(),
            provenance: designed("f11d.test.pod-rule"),
        },
        role(
            "beta.cockpit_b",
            PartRole::Cockpit,
            "f11d.test.beta-cockpit",
        ),
        role(
            "beta.wing_b",
            PartRole::ControlSurface,
            "f11d.test.beta-wing",
        ),
        role("beta.wing_b.gun_b", PartRole::Gun, "f11d.test.beta-gun"),
        SemanticBinding {
            path: "alpha.absent".to_owned(),
            role: known(PartRole::Engine),
            collision: known(CollisionRole::None),
            animation: Vec::new(),
            provenance: designed("f11d.test.absent-rule"),
        },
    ])
    .expect("the roster fixture rules name distinct paths")
}

/// The two containers the AC04 audit is asked about: the airframe container,
/// which converts, and a mission container whose node array no production
/// path decodes yet.
fn roster_containers() -> (ContentId, Vec<SceneContainerRef>) {
    let planes = cid(ContentKind::InstallFile, "fix_planes");
    let mission = cid(ContentKind::InstallFile, "fix_missions");
    let refs = vec![
        SceneContainerRef::new(planes.clone(), 13, 4_096),
        SceneContainerRef::new(mission.clone(), 733, 9_216),
    ];
    (planes, refs)
}

/// AC04's minimum scenario. The audit maps every root it can reach — both
/// airframe roots, every part, both gun mounts and the cockpit binding — with
/// each socket's role, collision role, zone, animation channels, provenance
/// and the node's one composed pose, and it names a typed blocker for the
/// airframes it cannot reach instead of reporting a pass.
#[test]
fn accept_f11_d_roster_audit_maps_every_root_part_mount_and_cockpit_binding() {
    let (planes, containers) = roster_containers();
    let mission_container = cid(ContentKind::InstallFile, "fix_missions");
    let scene = SceneGraph::build(
        &planes,
        &roster_container_fixture(),
        &fixture_adapter(),
        &roster_bindings(),
    )
    .expect("the roster fixture converts");
    let node = |path: &str| {
        SceneNodeId::from_content_id(cid(ContentKind::SceneNode, path)).expect("scene node id")
    };
    let root_of = |container: &ContentId, name: &str| {
        let key = format!("{}.{name}", container.key());
        SceneRootRef::new(
            container.clone(),
            SceneNodeId::from_content_id(cid(ContentKind::SceneNode, &key)).expect("scene node id"),
        )
        .expect("the reference names a root of its own container")
    };

    let alpha = cid(ContentKind::Airframe, "alpha");
    let beta = cid(ContentKind::Airframe, "beta");
    let ghost = cid(ContentKind::Airframe, "ghost");
    let orphan = cid(ContentKind::Airframe, "orphan");
    let wrong = cid(ContentKind::Airframe, "wrong");
    let mission = cid(ContentKind::Mission, "m07");

    let roster = AirframeRoster::new(
        vec![
            RosterEntry::new(alpha.clone(), designed("f11d.test.alpha-row"))
                .expect("an airframe row")
                .with_root(root_of(&planes, "alpha"))
                .with_availability(known(RosterAvailability::Selectable))
                .requiring(PartRole::Cockpit)
                .expect("cockpit is not required twice")
                .requiring(PartRole::Gun)
                .expect("gun is not required twice")
                .requiring(PartRole::Engine)
                .expect("engine is not required twice")
                .requiring(PartRole::DamageZone)
                .expect("damage zone is not required twice"),
            RosterEntry::new(beta.clone(), designed("f11d.test.beta-row"))
                .expect("an airframe row")
                .with_root(root_of(&planes, "beta"))
                .requiring(PartRole::Cockpit)
                .expect("cockpit is not required twice"),
            RosterEntry::new(ghost.clone(), designed("f11d.test.ghost-row"))
                .expect("an airframe row")
                .with_root(root_of(&mission_container, "shrike")),
            RosterEntry::new(orphan.clone(), designed("f11d.test.orphan-row"))
                .expect("an airframe row"),
            RosterEntry::new(wrong.clone(), designed("f11d.test.wrong-row"))
                .expect("an airframe row")
                .with_root(root_of(&planes, "nowhere")),
        ],
        vec![
            ForcedMissionAssignment::new(mission.clone(), beta.clone(), designed("f11d.test.m07"))
                .expect("a forced assignment"),
            ForcedMissionAssignment::new(
                cid(ContentKind::Mission, "m09"),
                beta.clone(),
                designed("f11d.test.m09"),
            )
            .expect("a forced assignment"),
        ],
    )
    .expect("the roster is internally consistent");

    let report = roster.audit(&containers, |container| {
        if *container == planes {
            Ok(&scene)
        } else {
            Err(ContainerBlocker::NodeArrayUndecoded {
                container: container.clone(),
                stored_nodes: 733,
                nodes_offset: 9_216,
            })
        }
    });

    // The container census: what each container's own header declared, kept
    // beside the verdict, so a blocked container still reports its size.
    assert_eq!(report.container_count(), 2);
    assert_eq!(report.airframe_count(), 5);
    let audited_planes = &report.containers()[0];
    assert_eq!(audited_planes.container(), &planes);
    assert_eq!(audited_planes.declared_nodes(), 13);
    assert_eq!(audited_planes.nodes_offset(), 4_096);
    assert!(audited_planes.is_mapped());
    assert!(matches!(
        audited_planes.outcome(),
        ContainerOutcome::Mapped(_)
    ));
    let mapping = audited_planes
        .mapping()
        .expect("the planes container converted");
    assert_eq!(mapping.node_count(), 13, "every stored record is a node");
    assert_eq!(mapping.roots().len(), 2, "alpha and beta");
    assert_eq!(
        mapping.airframes(),
        &[alpha.clone(), beta.clone(), wrong.clone()],
        "the container knows which discovered airframes live in it"
    );
    // The rule that matched nothing is a container shortfall, and nobody
    // silently dropped it.
    assert_eq!(
        audited_planes.gaps().cloned().collect::<Vec<AuditGap>>(),
        vec![AuditGap::UnmatchedRule {
            path: "alpha.absent".to_owned()
        }]
    );

    // The blocked container keeps its measured facts and says what is missing,
    // in numbers rather than in prose.
    let audited_missions = &report.containers()[1];
    assert!(!audited_missions.is_mapped());
    assert!(matches!(
        audited_missions.outcome(),
        ContainerOutcome::Blocked(_)
    ));
    assert_eq!(audited_missions.declared_nodes(), 733);
    assert_eq!(audited_missions.nodes_offset(), 9_216);
    assert_eq!(
        audited_missions.blocker(),
        Some(&ContainerBlocker::NodeArrayUndecoded {
            container: mission_container.clone(),
            stored_nodes: 733,
            nodes_offset: 9_216,
        })
    );
    let blocker_text = audited_missions.blocker().expect("a blocker").to_string();
    assert!(
        blocker_text.contains("733 stored node records") && blocker_text.contains("9216"),
        "the blocker quotes the measured record count and offset: {blocker_text}"
    );

    // The mapping arm: both reachable roots, every part, both gun mounts and
    // the cockpit binding, each by stable id and in stable-id order.
    assert_eq!(report.mapped_root_count(), 2);
    assert_eq!(
        report.mapped_socket_count(),
        10,
        "seven on alpha, three on beta"
    );
    let alpha_audit = report
        .airframes()
        .iter()
        .find(|audit| audit.airframe() == &alpha)
        .expect("alpha was audited");
    let alpha_map = alpha_audit.mapping().expect("alpha mapped");
    assert_eq!(alpha_map.root(), &node("fix_planes.alpha"));
    assert_eq!(alpha_map.container(), &planes);
    assert_eq!(
        alpha_map.node_count(),
        8,
        "alpha's own subtree, not the container's"
    );
    assert_eq!(
        alpha_map
            .sockets()
            .map(|socket| socket.node().key().to_owned())
            .collect::<Vec<_>>(),
        vec![
            "fix_planes.alpha.camera".to_owned(),
            "fix_planes.alpha.cockpit".to_owned(),
            "fix_planes.alpha.engine".to_owned(),
            "fix_planes.alpha.hull".to_owned(),
            "fix_planes.alpha.rocket".to_owned(),
            "fix_planes.alpha.wing_l".to_owned(),
            "fix_planes.alpha.wing_l.gun_l".to_owned(),
        ],
        "alpha's seven evidenced sockets, and nothing from the other root"
    );
    for (role, count) in [
        (PartRole::Cockpit, 1),
        (PartRole::ControlSurface, 1),
        (PartRole::Gun, 1),
        (PartRole::RocketMount, 1),
        (PartRole::Engine, 1),
        (PartRole::CameraAnchor, 1),
        (PartRole::DamageZone, 1),
    ] {
        assert_eq!(
            alpha_map.count_of(role),
            count,
            "alpha binds exactly {count} {} socket(s)",
            role.label()
        );
    }

    // A mapped socket carries the whole binding: the rule's provenance, the
    // stored zone, the bound animation channel — and the node's one composed
    // pose, so the mount point in the report is the pose the scene uses.
    let gun = alpha_map
        .sockets_of_role(PartRole::Gun)
        .next()
        .expect("alpha's gun");
    assert_eq!(gun.node(), &node("fix_planes.alpha.wing_l.gun_l"));
    assert_eq!(gun.collision(), CollisionRole::Collider);
    assert_eq!(
        gun.zone_id(),
        42,
        "the stored zone rides through the report"
    );
    assert_eq!(gun.animation_channels(), 1);
    assert_eq!(gun.provenance(), &designed("f11d.test.alpha-gun"));
    let gun_node = scene
        .node(&node("fix_planes.alpha.wing_l.gun_l"))
        .expect("the gun node");
    assert_ne!(
        gun.pose(),
        gun_node.local_transform(),
        "the fixture mounts the gun under a rotated, offset wing, so a socket \
         that copied the local transform would differ from the composed pose"
    );
    assert_eq!(gun.pose(), gun_node.world_transform());
    assert_eq!(gun.pose(), gun_node.collision_transform());
    assert_eq!(gun.pose(), scene.socket(gun.node()).expect("socket").pose());

    let cockpit = alpha_map
        .sockets_of_role(PartRole::Cockpit)
        .next()
        .expect("alpha's cockpit binding");
    assert_eq!(cockpit.node(), &node("fix_planes.alpha.cockpit"));
    assert_eq!(
        cockpit.provenance(),
        &designed("f11d.test.alpha-cockpit"),
        "the cockpit binding keeps its own rule's provenance"
    );

    // alpha is the one airframe the audit can call complete: every declared
    // role is bound, its availability is evidenced and nothing is missing.
    assert!(alpha_audit.is_complete());
    assert!(alpha_audit.is_proven_selectable());
    assert_eq!(
        alpha_audit.gaps().count(),
        0,
        "a complete airframe has no gaps: {:?}",
        alpha_audit.gaps().collect::<Vec<_>>()
    );
    assert_eq!(alpha_audit.forced_missions(), &[] as &[ContentId]);

    // A mission-only type: the audit mapped its root, and it still refuses to
    // call it selectable because the forced assignments say nothing about the
    // roster (F11 non-negotiable behavior 3).
    let beta_audit = report
        .airframes()
        .iter()
        .find(|audit| audit.airframe() == &beta)
        .expect("beta was audited");
    let beta_map = beta_audit.mapping().expect("beta mapped");
    assert_eq!(beta_map.node_count(), 5, "beta's own subtree");
    assert_eq!(beta_map.count_of(PartRole::Cockpit), 1);
    assert_eq!(beta_map.count_of(PartRole::Gun), 1);
    assert_eq!(beta_map.count_of(PartRole::Engine), 0);
    assert!(
        !beta_audit.is_proven_selectable(),
        "a forced mission assignment is not proof of selectability"
    );
    assert_eq!(
        beta_audit.availability(),
        &RosterEntry::undiscovered_availability(),
        "the availability stayed the explicit unknown it was declared as"
    );
    assert_eq!(
        beta_audit.forced_missions().len(),
        2,
        "both missions that force beta are recorded"
    );
    assert_eq!(&beta_audit.forced_missions()[0], &mission);
    assert_eq!(
        beta_audit.gaps().cloned().collect::<Vec<AuditGap>>(),
        vec![
            AuditGap::AvailabilityUndiscovered {
                airframe: beta.clone()
            },
            AuditGap::ForcedAssignmentOnly {
                airframe: beta.clone(),
                missions: 2
            },
            AuditGap::UnknownRole {
                node: node("fix_planes.beta.wing_b.gun_b.pod_b"),
                claim_id: claim("f11d.test.pod-role-unmeasured"),
                reason: "no evidence named the pod's gameplay role".to_owned()
            }
        ],
        "beta's roster availability is undiscovered, the missions are the only \
         evidence of it, and its pod's role was never evidenced"
    );
    assert!(
        !beta_map
            .sockets()
            .any(|socket| socket.node().key().ends_with("pod_b")),
        "an unmeasured role is a gap, never a mapped socket"
    );
    assert!(
        !beta_audit.is_complete(),
        "a mapped airframe with undiscovered roster availability is not complete"
    );

    // The airframe in the undecodable mission container inherits exactly that
    // blocker, carrying the container's own measured numbers.
    let ghost_audit = report
        .airframes()
        .iter()
        .find(|audit| audit.airframe() == &ghost)
        .expect("ghost was audited");
    assert!(ghost_audit.mapping().is_none());
    assert!(!ghost_audit.is_proven_selectable());
    assert_eq!(
        ghost_audit
            .blockers()
            .cloned()
            .collect::<Vec<AirframeBlocker>>(),
        vec![AirframeBlocker::ContainerUndecoded {
            airframe: ghost.clone(),
            blocker: ContainerBlocker::NodeArrayUndecoded {
                container: mission_container,
                stored_nodes: 733,
                nodes_offset: 9_216,
            },
        }]
    );
    assert!(
        ghost_audit
            .first_blocker()
            .expect("a blocker")
            .to_string()
            .contains("733 stored node records"),
        "the airframe blocker repeats the measured record count"
    );

    // An airframe with no discovered root, and one pointing at a root the
    // converted container does not hold: both are blockers, and neither falls
    // back to "the first root".
    let orphan_audit = report
        .airframes()
        .iter()
        .find(|audit| audit.airframe() == &orphan)
        .expect("orphan was audited");
    assert_eq!(
        orphan_audit
            .blockers()
            .cloned()
            .collect::<Vec<AirframeBlocker>>(),
        vec![AirframeBlocker::RootUndiscovered {
            airframe: orphan.clone()
        }]
    );
    let wrong_audit = report
        .airframes()
        .iter()
        .find(|audit| audit.airframe() == &wrong)
        .expect("wrong was audited");
    assert_eq!(
        wrong_audit
            .blockers()
            .cloned()
            .collect::<Vec<AirframeBlocker>>(),
        vec![AirframeBlocker::RootMissing {
            airframe: wrong.clone(),
            root: node("fix_planes.nowhere"),
        }]
    );

    // The report as a whole is not a pass, and its totals reconcile.
    assert!(!report.is_complete());
    assert!(!report.is_empty());
    assert_eq!(
        report.blocker_count(),
        4,
        "one container blocker and three airframe blockers"
    );
    assert_eq!(
        report.gap_count(),
        7,
        "one container rule and six airframe gaps"
    );
    assert_eq!(report.mapped_containers().count(), 1);
    assert_eq!(report.blocked_containers().count(), 1);
    assert_eq!(report.mapped_airframes().count(), 2);
    assert_eq!(report.blocked_airframes().count(), 3);

    // The roster itself keeps the two discoveries apart, and offers them by
    // either direction.
    assert_eq!(roster.entries().len(), 5);
    assert_eq!(roster.assignments().len(), 2);
    assert_eq!(
        roster.missions_forcing(&beta).cloned().collect::<Vec<_>>(),
        vec![mission.clone(), cid(ContentKind::Mission, "m09")]
    );
    assert_eq!(
        roster
            .airframes_forced_in(&mission)
            .cloned()
            .collect::<Vec<_>>(),
        vec![beta.clone()]
    );
    assert!(roster.entry(&alpha).is_some());
    assert!(
        roster
            .entry(&cid(ContentKind::Airframe, "absent"))
            .is_none()
    );
}

/// The shortfall fixture: a container whose header declares more stored node
/// records than any reader has decoded, a root that carries a socket whose
/// gameplay role is evidenced but whose collision role is not, and a
/// container the audit was never asked to cover.
fn partial_container_fixture() -> Vec<ParsedNode> {
    let mut nodes: Vec<ParsedNode> = Vec::new();
    let mut slot_of = std::collections::BTreeMap::new();
    let root = ParsedNode::new(0, "solo", ParsedNodeKind::World);
    slot_of.insert(0, 0);
    nodes.push(root);
    append(&mut nodes, &mut slot_of, 1, "odd", Some(0));
    append(&mut nodes, &mut slot_of, 2, "spare", Some(0));
    nodes
}

fn partial_bindings() -> BindingMap {
    BindingMap::new(vec![SemanticBinding {
        path: "solo.odd".to_owned(),
        role: known(PartRole::Engine),
        collision: unmeasured(
            "f11d.test.odd-collision-unmeasured",
            "the gameplay role is evidenced but nothing named the collision role",
        ),
        animation: Vec::new(),
        provenance: designed("f11d.test.odd-rule"),
    }])
    .expect("the shortfall fixture rules name distinct paths")
}

/// Every shortfall the audit can find, one at a time, and each of them blocks
/// `is_complete`: a container whose header declares far more node records than
/// were decoded, a rule that bound nothing, a socket with no established
/// role, a required role nothing bound, a root in a container the audit never
/// covered, and a container whose conversion was refused outright.
#[test]
fn accept_f11_d_roster_audit_reports_each_shortfall_instead_of_a_pass() {
    let partial = cid(ContentKind::InstallFile, "fix_partial");
    let uncovered = cid(ContentKind::InstallFile, "fix_uncovered");
    let refused = cid(ContentKind::InstallFile, "fix_refused");
    let scene = SceneGraph::build(
        &partial,
        &partial_container_fixture(),
        &fixture_adapter(),
        &partial_bindings(),
    )
    .expect("the shortfall fixture converts");
    let node = |path: &str| {
        SceneNodeId::from_content_id(cid(ContentKind::SceneNode, path)).expect("scene node id")
    };
    let root_of = |container: &ContentId, name: &str| {
        SceneRootRef::new(
            container.clone(),
            SceneNodeId::from_content_id(cid(
                ContentKind::SceneNode,
                &format!("{}.{name}", container.key()),
            ))
            .expect("scene node id"),
        )
        .expect("the reference names a root of its own container")
    };

    // The header claims 99 stored node records; three were decoded. A partial
    // decode must not read as a complete mapping.
    let containers = vec![
        SceneContainerRef::new(partial.clone(), 99, 2_048),
        SceneContainerRef::new(refused.clone(), 12, 4_096),
    ];
    let short = cid(ContentKind::Airframe, "short");
    let outside = cid(ContentKind::Airframe, "outside");
    let denied = cid(ContentKind::Airframe, "denied");
    let roster = AirframeRoster::new(
        vec![
            RosterEntry::new(short.clone(), designed("f11d.test.short-row"))
                .expect("an airframe row")
                .with_root(root_of(&partial, "solo"))
                .with_availability(known(RosterAvailability::Selectable))
                .requiring(PartRole::Cockpit)
                .expect("cockpit is not required twice")
                .requiring(PartRole::Gun)
                .expect("gun is not required twice"),
            RosterEntry::new(outside.clone(), designed("f11d.test.outside-row"))
                .expect("an airframe row")
                .with_root(root_of(&uncovered, "ghost"))
                .with_availability(known(RosterAvailability::MissionOnly)),
            RosterEntry::new(denied.clone(), designed("f11d.test.denied-row"))
                .expect("an airframe row")
                .with_root(root_of(&refused, "wreck")),
        ],
        Vec::new(),
    )
    .expect("the roster is internally consistent");

    let report = roster.audit(&containers, |container| match container.key() {
        "fix_partial" => Ok(&scene),
        _ => Err(ContainerBlocker::SceneRefused {
            container: container.clone(),
            reason: "the node array reader is not implemented".to_owned(),
        }),
    });

    // The container-level shortfalls: the declared record count the decode did
    // not reach, and the rule that bound nothing.
    let audited = &report.containers()[0];
    assert!(audited.is_mapped());
    assert_eq!(
        audited.gaps().cloned().collect::<Vec<AuditGap>>(),
        vec![AuditGap::NodeCountMismatch {
            container: partial.clone(),
            declared: 99,
            decoded: 3
        }]
    );
    assert!(
        audited
            .gaps()
            .any(|gap| matches!(gap, AuditGap::NodeCountMismatch { .. })),
        "a partial decode is reported, never rounded up to a pass"
    );
    // The refused container keeps its own blocker, quoted verbatim.
    let refused_audit = &report.containers()[1];
    assert!(!refused_audit.is_mapped());
    assert_eq!(
        refused_audit.blocker(),
        Some(&ContainerBlocker::SceneRefused {
            container: refused.clone(),
            reason: "the node array reader is not implemented".to_owned()
        })
    );

    // The airframe that did map still reports both its socket with no
    // established role and its two required roles nothing bound.
    let short_audit = report
        .airframes()
        .iter()
        .find(|audit| audit.airframe() == &short)
        .expect("short was audited");
    let short_map = short_audit.mapping().expect("short mapped");
    assert_eq!(short_map.node_count(), 3);
    assert_eq!(
        short_map.len(),
        0,
        "the one bound node has no established role"
    );
    assert_eq!(
        short_audit.gaps().cloned().collect::<Vec<AuditGap>>(),
        vec![
            AuditGap::UnknownRole {
                node: node("fix_partial.solo.odd"),
                claim_id: claim("f11d.test.odd-collision-unmeasured"),
                reason: "the gameplay role is evidenced but nothing named the \
                         collision role"
                    .to_owned()
            },
            AuditGap::MissingRole {
                root: node("fix_partial.solo"),
                role: PartRole::Cockpit
            },
            AuditGap::MissingRole {
                root: node("fix_partial.solo"),
                role: PartRole::Gun
            }
        ],
        "an unevidenced role, then each required role nothing bound"
    );
    assert!(!short_audit.is_complete());

    // An airframe in a container the audit was not asked about is a blocker,
    // not a silent omission.
    let outside_audit = report
        .airframes()
        .iter()
        .find(|audit| audit.airframe() == &outside)
        .expect("outside was audited");
    assert!(outside_audit.mapping().is_none());
    assert_eq!(
        outside_audit
            .blockers()
            .cloned()
            .collect::<Vec<AirframeBlocker>>(),
        vec![AirframeBlocker::ContainerNotAudited {
            airframe: outside.clone(),
            container: uncovered.clone()
        }]
    );
    assert!(
        !outside_audit.is_proven_selectable(),
        "a mission-only availability is not selectable"
    );

    // An airframe in a refused container inherits the refusal, with its own id.
    let denied_audit = report
        .airframes()
        .iter()
        .find(|audit| audit.airframe() == &denied)
        .expect("denied was audited");
    assert_eq!(
        denied_audit
            .blockers()
            .cloned()
            .collect::<Vec<AirframeBlocker>>(),
        vec![AirframeBlocker::ContainerUndecoded {
            airframe: denied.clone(),
            blocker: ContainerBlocker::SceneRefused {
                container: refused.clone(),
                reason: "the node array reader is not implemented".to_owned()
            }
        }]
    );

    // Nothing about the report is a pass.
    assert_eq!(report.container_count(), 2);
    assert_eq!(report.airframe_count(), 3);
    assert_eq!(report.mapped_root_count(), 1);
    assert_eq!(report.mapped_socket_count(), 0);
    assert_eq!(report.blocker_count(), 3, "one container and two airframes");
    assert_eq!(
        report.gap_count(),
        5,
        "one container gap, three under `short` and the undiscovered roster \
         availability of the refused `denied`"
    );
    assert!(!report.is_complete());
}

/// The roster is validated as one set, and a contradiction is refused at
/// construction rather than resolved by the audit: a non-airframe row, the
/// same airframe twice, a required role declared twice, an assignment naming
/// something that is not a mission, the same assignment twice, and — the
/// important one — a mission forcing an airframe no row audits, which would
/// otherwise make a roster with a hole look complete.
#[test]
fn accept_f11_d_roster_records_refuse_contradictions() {
    let alpha = cid(ContentKind::Airframe, "alpha");
    let other = cid(ContentKind::Airframe, "other");
    let mission = cid(ContentKind::Mission, "m01");
    let row = |airframe: ContentId| {
        RosterEntry::new(airframe, designed("f11d.test.row")).expect("an airframe row")
    };
    let assignment = |mission: ContentId, airframe: ContentId| {
        ForcedMissionAssignment::new(mission, airframe, designed("f11d.test.assignment"))
            .expect("a forced assignment")
    };

    // A row must audit an airframe.
    assert_eq!(
        RosterEntry::new(
            cid(ContentKind::Mesh, "fix_planes.1"),
            designed("f11d.test.row")
        ),
        Err(RosterError::AirframeKind {
            kind: ContentKind::Mesh
        })
    );
    // An assignment must name a mission and an airframe.
    assert_eq!(
        ForcedMissionAssignment::new(
            cid(ContentKind::World, "c1"),
            alpha.clone(),
            designed("f11d.test.assignment")
        ),
        Err(RosterError::MissionKind {
            kind: ContentKind::World
        })
    );
    assert_eq!(
        ForcedMissionAssignment::new(
            mission.clone(),
            cid(ContentKind::SceneNode, "fix_planes.alpha"),
            designed("f11d.test.assignment")
        ),
        Err(RosterError::AirframeKind {
            kind: ContentKind::SceneNode
        })
    );
    // A required role may not be declared twice.
    assert_eq!(
        row(alpha.clone())
            .requiring(PartRole::Gun)
            .expect("gun is required once")
            .requiring(PartRole::Gun)
            .err(),
        Some(RosterError::DuplicateRequiredRole {
            role: PartRole::Gun
        })
    );
    // The same airframe may not be audited twice.
    assert_eq!(
        AirframeRoster::new(vec![row(alpha.clone()), row(alpha.clone())], Vec::new()).err(),
        Some(RosterError::DuplicateAirframe {
            airframe: alpha.clone()
        })
    );
    // The same mission may not force the same airframe twice, but two
    // missions forcing one airframe is the normal case.
    assert_eq!(
        AirframeRoster::new(
            vec![row(alpha.clone()), row(other.clone())],
            vec![
                assignment(mission.clone(), alpha.clone()),
                assignment(mission.clone(), alpha.clone())
            ]
        )
        .err(),
        Some(RosterError::DuplicateAssignment {
            mission: mission.clone(),
            airframe: alpha.clone()
        })
    );
    assert!(
        AirframeRoster::new(
            vec![row(alpha.clone())],
            vec![assignment(mission.clone(), alpha.clone())]
        )
        .is_ok()
    );
    // A mission that forces a plane the roster does not audit is a discovery
    // gap and is refused, not folded into the roster.
    assert_eq!(
        AirframeRoster::new(
            vec![row(alpha.clone())],
            vec![assignment(mission.clone(), other.clone())]
        )
        .err(),
        Some(RosterError::UnknownAirframe { airframe: other })
    );

    // An empty roster audits nothing, and nothing audited is never a pass.
    let empty = AirframeRoster::new(Vec::new(), Vec::new()).expect("an empty roster is valid");
    let report = empty.audit(&[], |_| {
        Err(ContainerBlocker::SceneRefused {
            container: cid(ContentKind::InstallFile, "fix_planes"),
            reason: "unused".to_owned(),
        })
    });
    assert!(report.is_empty());
    assert!(
        !report.is_complete(),
        "an audit that looked at nothing must not read as a pass"
    );
    assert_eq!(report.container_count(), 0);
    assert_eq!(report.airframe_count(), 0);
}

// --------------------------------------------------- F11-D retail (AC04) ---

/// The nine GameZ archives of the original installation, with the
/// `nodes_offset` the pinned mech3ax v0.6.0 reference records for each.
///
/// These are the reference's own numbers, not this test's output, so a reader
/// or a census that mis-walks a container cannot pass by agreeing with
/// itself. The `node_array_size` column is *not* pinned here: the reference
/// documents the field, but no independent copy of the per-container values
/// exists in this repository, so the retail test asserts only what it can
/// check independently — the `nodes_offset` against this table, the mesh walk
/// landing on it, and a non-zero record count.
const RETAIL_GAMEZ_NODES_OFFSET: [(&str, u32); 9] = [
    ("zbd/planes.zbd", 4_881_228),
    ("zbd/c1/gamez.zbd", 4_326_296),
    ("zbd/c1b/gamez.zbd", 1_924_148),
    ("zbd/c1c/gamez.zbd", 1_964_684),
    ("zbd/c2/gamez.zbd", 3_111_828),
    ("zbd/c2b/gamez.zbd", 1_658_700),
    ("zbd/c3/gamez.zbd", 3_661_748),
    ("zbd/c4/gamez.zbd", 5_107_144),
    ("zbd/c5/gamez.zbd", 5_259_292),
];

/// The read-only original installation, or a loud failure when the `retail`
/// capability is missing. Never a silent skip: a test that cannot prove
/// anything must fail, not pass.
fn retail_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("CS_GAME_DIR").expect(
        "CS_GAME_DIR must point at the original installation: this test audits the private \
         airframe roster against real container bytes and cannot pass without them",
    ))
}

/// One measured GameZ container of the original installation.
struct GameZCensusRow {
    /// The case-insensitive logical key, e.g. `zbd/c5/gamez.zbd`.
    logical: String,
    /// The header's `node_array_size`.
    stored_nodes: u32,
    /// The header's `nodes_offset`.
    nodes_offset: u32,
    /// How many stored mesh records the container really holds.
    present_meshes: usize,
    /// The container's catalog key, through the production normalizer.
    catalog_key: String,
}

/// The census the F11-D roster audit runs over: every archive under the ZBD
/// root, offered to the production GameZ reader and measured.
///
/// The reader's own signature and version check is what classifies a
/// container, so no filename decides this. What the second entrypoint adds is
/// stated exactly, because it is weaker than it looks: both
/// `read_gamez_meshes` and `read_gamez_materials` parse the 40 header bytes
/// through the *same* `cs_formats::gamez::reader::read_container_header`, so
/// agreeing about `node_array_size` and `nodes_offset` is a consistency check
/// on two pipelines over one header, not a second independent parse. The
/// independent evidence for the offsets is the pinned reference table in
/// `RETAIL_GAMEZ_NODES_OFFSET`; what the mesh entrypoint adds is a *walk* that
/// has to end exactly on the `nodes_offset` the header declares, and what the
/// material entrypoint adds is a material section that has to end exactly on
/// `meshes_offset`. Rows come back in logical-key order.
fn retail_gamez_census(game_dir: &std::path::Path) -> Vec<GameZCensusRow> {
    use cs_assets::install as install_api;
    use cs_content::catalog::baseline::install_file_key;
    use cs_formats::gamez::{read_gamez_materials, read_gamez_meshes};
    use cs_formats::io::ParseContext;

    let found = install_api::discover(game_dir)
        .expect("production discovery must read the original installation");
    let mut census: Vec<GameZCensusRow> = Vec::new();
    for record in &found.manifest.files {
        let logical = record.relative_spelling.logical_key();
        if !logical.starts_with("zbd/") {
            continue;
        }
        let path = found
            .manifest
            .host_root
            .join(record.relative_spelling.as_str());
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|error| panic!("{logical}: the installation must hold it: {error}"));
        let mut context = ParseContext::with_defaults(logical.clone());
        let Ok(meshes) = read_gamez_meshes(&mut context, &logical, &bytes) else {
            continue;
        };
        let materials =
            read_gamez_materials(&mut context, &logical, &bytes).unwrap_or_else(|error| {
                panic!("{logical}: the retail material section must read: {error}")
            });
        // Both entrypoints share one header parser (see this function's doc),
        // so this is a consistency check, not a second parse; the mesh walk
        // below is the independent evidence that the node array really starts
        // where the header says.
        assert_eq!(
            (
                materials.header.node_array_size,
                materials.header.nodes_offset
            ),
            (meshes.header.node_array_size, meshes.header.nodes_offset),
            "{logical}: the two entrypoints disagree about the node array words"
        );
        assert_eq!(
            meshes.data_end,
            u64::from(meshes.header.nodes_offset),
            "{logical}: the mesh walk must end on the node array"
        );
        census.push(GameZCensusRow {
            catalog_key: install_file_key(&logical),
            logical,
            stored_nodes: meshes.header.node_array_size,
            nodes_offset: meshes.header.nodes_offset,
            present_meshes: meshes.present_count(),
        });
    }
    census.sort_by(|left, right| left.logical.cmp(&right.logical));
    census
}

/// AC04 over the real installation. Every GameZ archive under the installation's
/// ZBD root is discovered by production discovery, classified by the production
/// GameZ reader, and measured: how many stored node records each container's own
/// header declares and where the array starts. The `nodes_offset` values are
/// cross-checked against the pinned reference — the independent check — and each
/// container's mesh data walk has to end exactly on the offset its header
/// declares.
///
/// The verdict is the honest one, and it is the verdict this stage is for: no
/// production path decodes a GameZ node array (#392), so the audit maps **no**
/// root, part, mount or cockpit binding and names a blocker per container with
/// the measured record count. No airframe element has been discovered from
/// original data yet either, so the roster is empty — and an empty roster is
/// not a pass, it is a finding. What the test pins is that the corpus exists,
/// that the census is right, and that the audit reports the gap instead of
/// inventing a mapping.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f11_d_retail_the_private_installation_roster_audit_flags_every_container() {
    let census = retail_gamez_census(&retail_dir());

    // The corpus is the nine GameZ archives, and each one's node array starts
    // where the pinned reference records.
    let discovered: Vec<&str> = census.iter().map(|row| row.logical.as_str()).collect();
    let mut expected: Vec<&str> = RETAIL_GAMEZ_NODES_OFFSET
        .iter()
        .map(|(key, _)| *key)
        .collect();
    expected.sort_unstable();
    assert_eq!(
        discovered, expected,
        "the installation's GameZ corpus is the nine measured archives"
    );
    for row in &census {
        let key = &row.logical;
        let (_, reference_offset) = RETAIL_GAMEZ_NODES_OFFSET
            .iter()
            .find(|(name, _)| name == key)
            .unwrap_or_else(|| panic!("{key} is one of the nine measured archives"));
        assert_eq!(
            row.nodes_offset, *reference_offset,
            "{key}: the node array must start on the reference's recorded offset"
        );
        assert!(
            row.stored_nodes > 0,
            "{key}: a container with no stored node record holds no scene"
        );
        assert!(
            row.present_meshes > 0,
            "{key}: a container with no present mesh holds no geometry"
        );
    }
    let total_nodes: u32 = census.iter().map(|row| row.stored_nodes).sum();
    let planes_nodes = census
        .iter()
        .find(|row| row.logical == "zbd/planes.zbd")
        .map(|row| row.stored_nodes)
        .expect("the shared airframe archive is in the corpus");
    assert_eq!(
        total_nodes, 56_620,
        "the corpus declares 56,620 stored node records in total"
    );
    assert!(
        total_nodes > planes_nodes * 10,
        "the per-chapter mission archives hold far more scene records than the shared airframe \
         archive ({total_nodes} against {planes_nodes}), so mission-only airframes live in the \
         same undecoded section as the shared roster"
    );

    // The audit, over the real census. No production path decodes a node array
    // yet, so the graph source answers the measured blocker for every
    // container; the roster is empty because no airframe element has been
    // discovered from original data.
    let containers: Vec<SceneContainerRef> = census
        .iter()
        .map(|row| {
            SceneContainerRef::new(
                // The catalog identity of the container, through the
                // production key normalizer: a relative spelling is not a
                // content key.
                cid(ContentKind::InstallFile, &row.catalog_key),
                row.stored_nodes,
                row.nodes_offset,
            )
        })
        .collect();
    let roster = AirframeRoster::new(Vec::new(), Vec::new()).expect("an empty roster is valid");
    let report = roster.audit(&containers, |container| {
        let reference = containers
            .iter()
            .find(|reference| reference.container() == container)
            .expect("the audit only asks about the containers it was given");
        Err(ContainerBlocker::NodeArrayUndecoded {
            container: container.clone(),
            stored_nodes: reference.stored_nodes(),
            nodes_offset: reference.nodes_offset(),
        })
    });

    assert_eq!(
        report.container_count(),
        9,
        "every discovered container audited"
    );
    assert_eq!(report.mapped_containers().count(), 0);
    assert_eq!(report.blocked_containers().count(), 9);
    assert_eq!(report.mapped_root_count(), 0, "no root could be mapped");
    assert_eq!(report.mapped_socket_count(), 0, "no mount or cockpit bound");
    assert_eq!(
        report.airframe_count(),
        0,
        "no airframe element is discovered yet"
    );
    // Nine blocked containers and no airframe row at all: the report is not
    // empty (it audited something) and it is not a pass.
    assert!(!report.is_empty(), "nine containers were audited");
    assert!(
        !report.is_complete(),
        "an audit that mapped nothing is never a pass"
    );
    assert_eq!(report.blocker_count(), 9, "one blocker per container");
    for (audit, row) in report.containers().iter().zip(&census) {
        assert_eq!(audit.container().key(), row.catalog_key);
        assert_eq!(
            audit.declared_nodes(),
            row.stored_nodes,
            "{}: the measured count",
            row.logical
        );
        assert_eq!(
            audit.nodes_offset(),
            row.nodes_offset,
            "{}: the measured offset",
            row.logical
        );
        let blocker = audit.blocker().expect("every container is blocked");
        let text = blocker.to_string();
        assert!(
            text.contains(&format!("{} stored node records", row.stored_nodes))
                && text.contains(&row.nodes_offset.to_string()),
            "{}: the blocker must quote the measured facts, got {text}",
            row.logical
        );
    }
}

// ------------------------------------------------- F11-D evidence harness ---

/// The evidence-report harness for task F11-D
/// (`docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`).
///
/// This test is deliberately **not** named `accept_f11_d_*`: it is not part of
/// the acceptance suite, and it fails loudly when its inputs are missing
/// instead of passing vacuously. Run from the workspace root, after the
/// acceptance suite, exactly as:
///
/// 1. ```sh
///    mkdir -p private/evidence/F11-D
///    cargo test --workspace --locked -- accept_f11_d_ --include-ignored \
///      2>&1 | tee private/evidence/F11-D/cargo-test.log
///    ```
///    (record the pipeline's exit status; it is passed to this harness as
///    `CS_EVIDENCE_EXIT_CODE`.)
/// 2. ```sh
///    CS_EVIDENCE_DIR=private/evidence/F11-D \
///    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
///    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f11_d_ --include-ignored" \
///    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
///      cargo test --locked -p cs_content --test scene -- evidence_report_f11_d_ --ignored
///    ```
/// 3. ```sh
///    python3 tools/validate_evidence.py private/evidence/F11-D/acceptance.json \
///      --artifact-root private/evidence/F11-D --require-pass
///    ```
/// 4. Commit a copy of `acceptance.json` as
///    `docs/findings/evidence/F11-D.json`.
///
/// Every field is derived here from real inputs: the recorded test log, the
/// environment, production discovery of `$CS_GAME_DIR`, the same
/// [`retail_gamez_census`] the retail acceptance test measures, `rustc
/// --version` and `Cargo.lock`. Nothing is typed in by hand except two texts:
/// the `review` block (which `CS_EVIDENCE_REVIEW` fills in for the reviewing
/// agent, and which otherwise says review is still pending) and the
/// product-coverage limitations it quotes.
///
/// `unknowns` is `[]` and the report validates with `--require-pass`: the
/// **task's** acceptance is complete — the audit exists, it runs over the real
/// corpus and it reports the measured blocker instead of inventing a mapping,
/// and every selected test passed. `tools/validate_evidence.py` rejects a
/// report whose `unknowns` hold unresolved *task* issues, so the
/// product-incompleteness state is moved, never deleted (2026-09-28 owner
/// directive): it lives in the `roster-census.json` artifact this report
/// hashes, in `review.method`, in `docs/findings/` and in the follow-up tasks
/// it names. A failing run produces a failing report, which the validator
/// rejects.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_f11_d_writes_the_acceptance_report() {
    use std::path::PathBuf;

    use cs_assets::install as install_api;

    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    // The candidate tree must be the tree that was actually tested: a stale
    // report from another commit is exactly what this check refuses.
    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; old \
         reports cannot be reused for new code"
    );

    // The acceptance suite is the evidence: parse its recorded output.
    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_f11_d_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_f11_d_` tests were recorded in {}",
        log_path.display()
    );

    // Capability coverage is checked, never assumed: `retail` is declared only
    // because the retail acceptance test is in this log.
    let retail = suite
        .assertions
        .iter()
        .find(|(name, _)| name.contains("accept_f11_d_retail_"))
        .unwrap_or_else(|| {
            panic!(
                "the retail acceptance test did not run: F11-D requires capability `retail`, run \
                 step 1 with `--include-ignored` and CS_GAME_DIR set"
            )
        });
    assert_eq!(retail.1, "pass", "the retail acceptance test must pass");
    assert!(
        suite
            .assertions
            .iter()
            .any(|(name, _)| name.contains("accept_f11_d_")
                && !name.contains("accept_f11_d_retail_")),
        "synthetic task tests must be present alongside the retail one"
    );

    // `source` hashes describe the real installation, measured by production
    // discovery.
    let found = install_api::discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = install_api::fingerprint(&found.manifest).to_hex();
    let content_sha256 = install_api::content_fingerprint(&found.manifest).to_hex();

    // The consumer trace: the same census the retail acceptance test measures,
    // plus the audit's verdict over it, written into the evidence directory.
    // The rerun re-checks the reference cross-check, so the artifact cannot be
    // written from numbers nobody verified.
    let census = retail_gamez_census(&game_dir);
    assert_eq!(
        census.len(),
        RETAIL_GAMEZ_NODES_OFFSET.len(),
        "the census must cover every measured GameZ archive"
    );
    let containers: Vec<SceneContainerRef> = census
        .iter()
        .map(|row| {
            SceneContainerRef::new(
                cid(ContentKind::InstallFile, &row.catalog_key),
                row.stored_nodes,
                row.nodes_offset,
            )
        })
        .collect();
    let roster = AirframeRoster::new(Vec::new(), Vec::new()).expect("an empty roster is valid");
    let report = roster.audit(&containers, |container| {
        let reference = containers
            .iter()
            .find(|reference| reference.container() == container)
            .expect("the audit only asks about the containers it was given");
        Err(ContainerBlocker::NodeArrayUndecoded {
            container: container.clone(),
            stored_nodes: reference.stored_nodes(),
            nodes_offset: reference.nodes_offset(),
        })
    });
    let total_nodes: u32 = census.iter().map(|row| row.stored_nodes).sum();
    let census_path = evidence_dir.join("roster-census.json");
    let census_json = census_report_json(&census, &report, total_nodes, &install_sha256);
    fs::write(&census_path, &census_json)
        .unwrap_or_else(|error| panic!("write {}: {error}", census_path.display()));
    for needle in [
        "\"schema\":\"cs-scene-roster-audit/1\"",
        "\"retail\":true",
        "\"airframes_discovered\":0",
        "\"mapped_roots\":0",
        "\"mapped_sockets\":0",
        "\"blockers\":9",
        "\"complete\":false",
        &format!("\"install_sha256\":\"{install_sha256}\""),
        &format!("\"total_stored_nodes\":{total_nodes}"),
    ] {
        assert!(
            census_json.contains(needle),
            "the consumer report is missing {needle:?}"
        );
    }
    assert!(
        !census_json.contains("\"catalog_key\":\"fix_"),
        "the retail consumer report holds no authored row"
    );

    let engine = format!(
        "{{\"rust\": {}, \"bevy\": {}, \"avian\": {}}}",
        jstr(&rustc_version()),
        jstr(&locked_version("bevy")),
        jstr(&locked_version("avian3d"))
    );
    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&census_path, "json", &evidence_dir),
    ];

    let review = std::env::var("CS_EVIDENCE_REVIEW").unwrap_or_else(|_| {
        "pending: written by the implementing agent bunny-2. Rally assigns the reviewing agent, \
         who must regenerate this report on the reviewed and rebased commit and replace this text \
         with their own identity and method (CS_EVIDENCE_REVIEW); the reviewer is a different \
         agent identity from the implementer, and no agent review awards more than `checked`. \
         Method: the acceptance suite ran locally with the retail capability over $CS_GAME_DIR, \
         the consumer trace is the production GameZ census and the production roster audit over \
         the same installation, and tools/validate_evidence.py --require-pass checks the report."
            .to_owned()
            + &F11_D_LIMITATIONS
                .iter()
                .map(|limitation| format!(" LIMITATION: {limitation}"))
                .collect::<String>()
    });

    let report_json = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"F11-D\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \
         \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [{}],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine,
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        "",
        jstr(&review),
        jstr(
            "acceptance suite run locally with the retail capability; this harness derives every \
             field from the recorded log, production discovery of $CS_GAME_DIR, the production \
             GameZ census and roster audit over that installation, rustc and Cargo.lock; validated \
             with tools/validate_evidence.py --require-pass. The consumer trace is \
             cs_content::scene::AirframeRoster::audit over the containers cs_formats::gamez's two \
             production readers measured, with cs_content::catalog::baseline::install_file_key as \
             the container identity; it is a library path, and no cs-inspect subcommand wraps it \
             yet. Regenerated by the reviewing agent on the reviewed and rebased commit, as \
             docs/contracts/CLI-EVIDENCE.md requires."
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report_json)
        .unwrap_or_else(|error| panic!("write {}: {error}", out.display()));
    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"F11-D\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
        "\"unknowns\": [],",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written honestly \
         and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// The product-coverage limits this stage records instead of guessing, each
/// naming the affected content and the task that resolves it (2026-09-28 owner
/// directive: a limitation must survive into machine-readable evidence). They
/// are quoted in `review.method` and hashed inside the `roster-census.json`
/// artifact, never deleted to make a validator pass.
const F11_D_LIMITATIONS: &[&str] = &[
    "No production path decodes a GameZ node array, so the roster audit maps no root, part, mount \
     or cockpit binding from the original installation: the nine measured GameZ archives declare \
     56,620 stored node records in total and none of them is decoded. Affected content: every \
     airframe in the game, and every mission-only airframe that lives in a per-chapter gamez.zbd. \
     Resolving task: #392 (Read the GameZ node array into ParsedNode records), which needs the \
     owner to grant crates/cs_formats/ owner paths. This limitation gates every scene-hierarchy \
     and roster fidelity claim and survives this task being marked done.",
    "No airframe catalog element has been discovered from original data, so the roster the audit \
     takes is empty and no roster row can be audited. Affected content: the player-selectable \
     roster, the forced mission assignments and every airframe's mount and cockpit bindings. \
     Resolving task: #399 (Discover the airframe roster and its selectability from original \
     data), which depends on #392 and needs a decoded node array before a node name can be bound \
     to an airframe element.",
    "Roster availability is a designed vocabulary with no measured original meaning: which modes \
     let a player choose which airframe has not been observed, and a model name is still not \
     proof. Affected content: the selectable roster in every mode. Resolving tasks: #399 together \
     with the F22/F49 mode and preset stages.",
    "The F11-D audit is a library path: nothing in the running binary or in cs-inspect invokes it \
     yet, so no consumer trace exists outside the acceptance suite. Affected content: the audit's \
     own reachability. Resolving task: the F11-E producer (#398) that inserts the airframe scene \
     request, which should refuse an airframe the audit could not map.",
];

// ------------------------------------------------------- harness helpers ---

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (the F11-D evidence section of crates/cs_content/tests/scene.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path written relative to the workspace root must be re-anchored.
fn workspace_path(as_described: &str) -> std::path::PathBuf {
    let path = std::path::PathBuf::from(as_described);
    if path.is_absolute() {
        return path;
    }
    std::path::Path::new(&git(&["rev-parse", "--show-toplevel"])).join(path)
}

fn git(args: &[&str]) -> String {
    let output = Command::new("git").args(args).output().expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn rustc_version() -> String {
    let output = Command::new("rustc")
        .arg("--version")
        .output()
        .expect("rustc runs");
    assert!(output.status.success(), "rustc --version failed");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// The locked version of one `Cargo.lock` package: read, never asserted from
/// memory.
fn locked_version(package: &str) -> String {
    // Cargo runs the test binary from the package root, so the lock file is
    // located through git rather than through the package layout.
    let lock_path = Path::new(&git(&["rev-parse", "--show-toplevel"])).join("Cargo.lock");
    let lock = fs::read_to_string(&lock_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", lock_path.display()));
    let mut wanted = false;
    for line in lock.lines() {
        let line = line.trim();
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

/// What the recorded `cargo test` output says actually happened for this
/// task's prefix.
#[derive(Debug, Default)]
struct F11DSuite {
    discovered: u64,
    executed: u64,
    passed: u64,
    failed: u64,
    ignored: u64,
    /// `(test name, "pass" | "fail")`, in log order, deduplicated.
    assertions: Vec<(String, &'static str)>,
}

fn parse_f11_d_suite(log: &str) -> F11DSuite {
    use std::collections::VecDeque;
    let mut suite = F11DSuite::default();
    let mut pending: VecDeque<String> = VecDeque::new();
    for line in log.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("test result:") {
            for (count, kind) in summary_fields(trimmed) {
                match kind {
                    "passed" => suite.passed += count,
                    "failed" => suite.failed += count,
                    "ignored" => suite.ignored += count,
                    _ => {}
                }
            }
            continue;
        }
        if pending.front().is_some() && (trimmed == "ok" || trimmed == "FAILED") {
            let name = pending.pop_front().expect("pending test");
            record_result(
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
            if !name.contains("accept_f11_d_") {
                continue;
            }
            match tail.split_whitespace().next() {
                Some("ok") => record_result(&mut suite, name, "pass"),
                Some("FAILED") => record_result(&mut suite, name, "fail"),
                _ => pending.push_back(name),
            }
        }
    }
    suite.assertions.dedup_by(|left, right| left.0 == right.0);
    suite.executed = suite.passed + suite.failed;
    suite.discovered = suite.passed + suite.failed + suite.ignored;
    suite
}

fn summary_fields(line: &str) -> Vec<(u64, &str)> {
    let mut fields = Vec::new();
    for segment in line["test result:".len()..].split(';') {
        let words: Vec<&str> = segment.split_whitespace().collect();
        for pair in words.windows(2) {
            if let Ok(count) = pair[0].parse::<u64>()
                && matches!(pair[1], "passed" | "failed" | "ignored")
            {
                fields.push((count, pair[1]));
                break;
            }
        }
    }
    fields
}

fn record_result(suite: &mut F11DSuite, name: String, status: &'static str) {
    if suite.assertions.iter().any(|(seen, _)| *seen == name) {
        return;
    }
    suite.assertions.push((name, status));
}

/// The consumer-trace artifact: the measured census and the audit's verdict
/// over it. Only counts, offsets and digests — never original content.
fn census_report_json(
    census: &[GameZCensusRow],
    report: &cs_content::scene::RosterAuditReport,
    total_nodes: u32,
    install_sha256: &str,
) -> String {
    let rows: Vec<String> = census
        .iter()
        .map(|row| {
            format!(
                "{{\"logical\": {}, \"catalog_key\": {}, \"stored_nodes\": {}, \"nodes_offset\": \
                 {}, \"present_meshes\": {}, \"mapped\": false}}",
                jstr(&row.logical),
                jstr(&row.catalog_key),
                row.stored_nodes,
                row.nodes_offset,
                row.present_meshes
            )
        })
        .collect();
    format!(
        "{{\"schema\":\"cs-scene-roster-audit/1\",\"retail\":true,\"install_sha256\":{},\
         \"containers\":[{}],\"total_stored_nodes\":{},\"airframes_discovered\":{},\
         \"mapped_roots\":{},\"mapped_sockets\":{},\"blockers\":{},\"gaps\":{},\"complete\":{}}}",
        jstr(install_sha256),
        rows.join(","),
        total_nodes,
        report.airframe_count(),
        report.mapped_root_count(),
        report.mapped_socket_count(),
        report.blocker_count(),
        report.gap_count(),
        report.is_complete()
    )
}

/// One referenced artifact: hashed here with the production SHA-256 the
/// sibling crate implements (the validator re-hashes it with `hashlib`
/// independently).
fn artifact(source: &Path, kind: &str, evidence_dir: &Path) -> (String, String, String) {
    let name = source
        .file_name()
        .expect("artifact has a file name")
        .to_string_lossy()
        .into_owned();
    let target = evidence_dir.join(&name);
    if source != target {
        fs::copy(source, &target).unwrap_or_else(|error| {
            panic!("copy {} -> {}: {error}", source.display(), target.display())
        });
    }
    let bytes =
        fs::read(&target).unwrap_or_else(|error| panic!("read {}: {error}", target.display()));
    (name, sha256(&bytes).to_hex(), kind.to_owned())
}

fn assertion_array(assertions: &[(String, &'static str)]) -> String {
    assertions
        .iter()
        .map(|(name, status)| {
            format!(
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\"]}}",
                jstr(name)
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn artifact_array(artifacts: &[(String, String, String)]) -> String {
    artifacts
        .iter()
        .map(|(name, digest, kind)| {
            format!(
                "{{\"path\": {}, \"sha256\": {digest:?}, \"kind\": {kind:?}}}",
                jstr(name)
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn str_array(items: &[String]) -> String {
    format!(
        "[{}]",
        items
            .iter()
            .map(|item| jstr(item))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// A JSON string literal: quoted and escaped, so no report field can break out
/// of its string.
fn jstr(value: &str) -> String {
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

/// RFC 3339 with whole seconds and `Z`, which `datetime.fromisoformat`
/// accepts after the validator's `Z` → `+00:00` replacement.
fn iso_utc_now() -> String {
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock is after 1970")
        .as_secs() as i64;
    let (year, month, day, hour, minute, second) = civil_from_unix(epoch);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to a UTC
/// calendar date, because `std` has no date formatting.
fn civil_from_unix(seconds: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let year_of_day = year_of_era + era * 400;
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = (if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    }) as u32;
    let year = if month <= 2 {
        year_of_day + 1
    } else {
        year_of_day
    };
    (
        year,
        month,
        day,
        (rest / 3_600) as u32,
        ((rest % 3_600) / 60) as u32,
        (rest % 60) as u32,
    )
}

/// The production SHA-256, so the harness hashes with the same implementation
/// the rest of the engine uses.
fn sha256(bytes: &[u8]) -> cs_types::evidence::ContentHash {
    cs_assets::install::sha256(bytes)
}

// ===========================================================================
// Task #392: the GameZ node array decoded into `ParsedNode` records
// ===========================================================================
//
// The synthetic container below is written from the layout worksheet in
// `docs/findings/2026-10-02-gamez-node-array-layout.md` — the 40-byte header,
// the 212-byte info slot and the per-kind data records — by a writer that
// shares no code with the reader. The expected values are literals, so a reader
// and a writer that made the same mistake cannot agree.
//
// The `#[ignore]`d test at the end reads the read-only original installation
// and fails loudly without `$CS_GAME_DIR`.

/// The GameZ signature a Crimson Skies container stores.
const FIXTURE_SIGNATURE: u32 = 0x0297_1222;
/// The Crimson Skies container version.
const FIXTURE_VERSION: u32 = 42;
/// The texture table's offset, which the layout requires to be the header size.
const FIXTURE_TEXTURES_OFFSET: u32 = 40;
/// The material section's offset in the fixture: one word past the texture one.
const FIXTURE_MATERIALS_OFFSET: u32 = 44;
/// The mesh section's offset in the fixture: one word past the material one.
const FIXTURE_MESHES_OFFSET: u32 = 48;
/// Where the fixture's node array starts.
const FIXTURE_NODES_OFFSET: u32 = 52;

/// One object record's stored transform fields.
#[derive(Clone, Copy)]
struct ObjectSpec {
    flags: u32,
    rotation: [f32; 3],
    scale: [f32; 3],
    matrix: [[f32; 3]; 3],
    translation: [f32; 3],
}

/// The layout stores the euler triple in the source's declared angle unit, and
/// the pinned reference composes its stored matrix with `sin`/`cos` of the
/// **raw** numbers. So a record's matrix is only consistent with its euler triple
/// in the unit the source declares, and a fixture that mixes the two writes a
/// record that genuinely disagrees.
///
/// These are the two declared units the fixtures use: the reference's own
/// radian composition, and the F16-A fixture source's declared degrees.
fn composed_matrix(angle_unit_in_degrees: bool, rotation: [f32; 3]) -> [[f32; 3]; 3] {
    let scale = if angle_unit_in_degrees {
        std::f32::consts::PI / 180.0
    } else {
        1.0
    };
    let [x, y, z] = rotation.map(|angle| -angle * scale);
    let (sin_x, cos_x) = x.sin_cos();
    let (sin_y, cos_y) = y.sin_cos();
    let (sin_z, cos_z) = z.sin_cos();
    [
        [
            cos_y * cos_z,
            sin_x * sin_y * cos_z - cos_x * sin_z,
            cos_x * sin_y * cos_z + sin_x * sin_z,
        ],
        [
            cos_y * sin_z,
            sin_x * sin_y * sin_z + cos_x * cos_z,
            cos_x * sin_y * sin_z - sin_x * cos_z,
        ],
        [-sin_y, sin_x * cos_y, cos_x * cos_y],
    ]
}

impl ObjectSpec {
    /// A record the layout stores with a transform whose euler triple is in
    /// **radians**, the unit the pinned reference composes with.
    ///
    /// The writer composes the stored `matrix` from the worksheet's convention
    /// rather than writing a literal, because the record under test is the field
    /// layout and not the composition: `euler_matrix` is the reader's job and its
    /// own discriminating test lives in the F11-A suite. What the fixture must
    /// not do is write a matrix that *disagrees* while claiming agreement, so the
    /// disagreement case below stores its differing matrix explicitly.
    fn transformed(rotation: [f32; 3], translation: [f32; 3]) -> Self {
        Self {
            flags: 32,
            rotation,
            scale: [1.0, 1.0, 1.0],
            matrix: composed_matrix(false, rotation),
            translation,
        }
    }

    /// A record whose stored `matrix` deliberately disagrees with the one its own
    /// euler triple derives, which is what the reference's ~0.74 % corpus does.
    fn disagreeing(rotation: [f32; 3], matrix: [[f32; 3]; 3], translation: [f32; 3]) -> Self {
        Self {
            flags: 32,
            rotation,
            scale: [1.0, 1.0, 1.0],
            matrix,
            translation,
        }
    }

    /// A record the layout stores with no transform at all.
    const fn identity() -> Self {
        Self {
            flags: 40,
            rotation: [0.0; 3],
            scale: [1.0; 3],
            matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            translation: [0.0; 3],
        }
    }
}

/// One LOD record's stored fields.
#[derive(Clone, Copy)]
struct LodSpec {
    level: u32,
    range_near_sq: f32,
    range_far: f32,
    range_far_sq: f32,
    unk64: f32,
    unk72: f32,
}

/// The `range_far` / `range_far_sq` pair the layout stores twice.
fn lod(level: u32, near: f32, far: f32) -> LodSpec {
    LodSpec {
        level,
        range_near_sq: near * near,
        range_far: far,
        range_far_sq: far * far,
        unk64: 0.0,
        unk72: 0.0,
    }
}

/// A world record's size-determining fields: the grid and how many values each
/// cell stores.
#[derive(Clone, Copy)]
struct WorldSpec {
    partition_x_count: u32,
    partition_y_count: u32,
    values_per_cell: u16,
    own_children_count: u32,
}

/// One node the fixture writes.
struct NodeSpec {
    name: String,
    kind: u32,
    flags: u32,
    zone_id: u32,
    mesh_index: i32,
    parent: Option<u32>,
    children: Vec<u32>,
    /// Replaces the offset the writer would compute, so a test can make a
    /// record point somewhere the walk does not reach.
    data_ptr_override: Option<u32>,
    object: Option<ObjectSpec>,
    lod: Option<LodSpec>,
    world: Option<WorldSpec>,
    node_index: u32,
    unk196: u32,
    parent_count_override: Option<u16>,
    /// Replaces the record length the *writer* lays out, so a test can store a
    /// record whose stored counts are impossible without allocating for them.
    /// The reader still derives the length from the stored counts, so the walk
    /// and the buffer disagree — which is the point.
    written_len: Option<usize>,
}

impl NodeSpec {
    fn new(name: &str, kind: u32) -> Self {
        Self {
            name: name.to_owned(),
            kind,
            flags: 0x0180_0000,
            zone_id: 255,
            mesh_index: -1,
            parent: None,
            children: Vec::new(),
            data_ptr_override: None,
            object: None,
            lod: None,
            world: None,
            node_index: 0x0200_0000,
            unk196: 160,
            parent_count_override: None,
            written_len: None,
        }
    }

    /// Makes the writer lay out `written` bytes for this record, whatever the
    /// record's own stored counts imply — the shape a hostile container has, and
    /// the reason the reader must derive the length from the counts rather than
    /// trust the buffer.
    fn declared_len(mut self, written: usize) -> Self {
        self.written_len = Some(written);
        self
    }

    fn object(mut self, spec: ObjectSpec) -> Self {
        self.object = Some(spec);
        self
    }

    fn lod(mut self, spec: LodSpec) -> Self {
        self.lod = Some(spec);
        self
    }

    fn world(mut self, spec: WorldSpec) -> Self {
        self.world = Some(spec);
        self
    }

    fn parent(mut self, parent: u32) -> Self {
        self.parent = Some(parent);
        self
    }

    fn children(mut self, children: &[u32]) -> Self {
        self.children = children.to_vec();
        self
    }

    fn mesh(mut self, index: i32) -> Self {
        self.mesh_index = index;
        self
    }

    fn zone(mut self, zone: u32) -> Self {
        self.zone_id = zone;
        self
    }

    fn node_index(mut self, word: u32) -> Self {
        self.node_index = word;
        self
    }

    fn field196(mut self, value: u32) -> Self {
        self.unk196 = value;
        self
    }

    fn parent_count(mut self, value: u16) -> Self {
        self.parent_count_override = Some(value);
        self
    }

    fn data_ptr(mut self, offset: u32) -> Self {
        self.data_ptr_override = Some(offset);
        self
    }

    /// How many bytes the writer lays out for this record: the override when one
    /// is set, else what the stored counts imply.
    fn written_len(&self) -> usize {
        self.written_len.unwrap_or_else(|| self.data_len())
    }

    /// How many bytes this node's own data record occupies: the kind's fixed
    /// record, the world's variable block, the parent word and the child slots.
    fn data_len(&self) -> usize {
        let fixed = match self.kind {
            2 => {
                let Some(world) = self.world else {
                    panic!("a world record needs its grid");
                };
                let cells = usize::try_from(world.partition_x_count)
                    .expect("the fixture grid fits")
                    * usize::try_from(world.partition_y_count).expect("the fixture grid fits");
                // 204 header bytes, one child-value word, the grid and the slots.
                204 + 4 + cells * (88 + usize::from(world.values_per_cell) * 12)
            }
            // `7` is not a CS node type; the fixture writes it with the object
            // record's length so a refusal is the tag's own, not the walk's.
            5 | 7 => 144,
            6 => 92,
            1 => 488,
            3 => 248,
            4 => 28,
            9 => 256,
            // Any tag outside the layout is laid out with the object record's
            // length, so that the reader's refusal is the tag's own rather than
            // a walk that stopped at the wrong place.
            _ => 144,
        };
        fixed + 4 * usize::from(self.parent_word_is_present()) + 4 * self.children.len()
    }

    /// Whether the record stores the parent word at all.
    ///
    /// A LOD record always does, because a LOD variant cannot stand alone. A
    /// **light** record always does too, even though its own `parent_count`
    /// boolean is clear: the reference reads that word "as a result of
    /// `parent_count`, but is always 0", so the word is there and has to be
    /// consumed. Every other kind follows the `parent_count` boolean.
    fn parent_word_is_present(&self) -> bool {
        self.parent.is_some() || matches!(self.kind, 6 | 9)
    }
}

/// Writes one CS GameZ container holding exactly `nodes`.
///
/// The header, the 212-byte info slot and the per-kind data records are written
/// from the worksheet's field offsets, independently of the reader. Bytes after
/// the node array are none: the data section has to end exactly at the
/// container's end, which is one of the checks the reader makes.
fn write_container(nodes: &[NodeSpec]) -> Vec<u8> {
    if nodes.is_empty() {
        panic!(
            "the fixture always declares at least one node; an empty array is written by patching a written one"
        );
    }
    // The data section starts where the fixed-stride info array ends, so its
    // per-node offsets are computable before anything is written.
    let mut data_offset = FIXTURE_NODES_OFFSET + 212 * nodes.len() as u32;
    let mut offsets = Vec::with_capacity(nodes.len());
    for node in nodes {
        offsets.push(data_offset);
        data_offset += node.written_len() as u32;
    }

    let mut bytes = vec![0u8; data_offset as usize];
    let word = |bytes: &mut Vec<u8>, at: usize, value: u32| {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    };
    let half = |bytes: &mut Vec<u8>, at: usize, value: u16| {
        bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
    };
    let float = |bytes: &mut Vec<u8>, at: usize, value: f32| {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    };

    for (field, value) in [
        (0usize, FIXTURE_SIGNATURE),
        (4, FIXTURE_VERSION),
        (8, 0x1234_5678),
        (12, 1),
        (16, FIXTURE_TEXTURES_OFFSET),
        (20, FIXTURE_MATERIALS_OFFSET),
        (24, FIXTURE_MESHES_OFFSET),
        (28, nodes.len() as u32),
        (32, 0),
    ] {
        word(&mut bytes, field, value);
    }
    // `nodes_offset` is the last header word and depends on the array's size.
    word(&mut bytes, 36, FIXTURE_NODES_OFFSET);

    for (index, node) in nodes.iter().enumerate() {
        let at = FIXTURE_NODES_OFFSET as usize + 212 * index;
        let name = node.name.as_bytes();
        assert!(
            name.len() < 36,
            "the fixture's names fit their 36-byte field"
        );
        bytes[at..at + name.len()].copy_from_slice(name);
        word(&mut bytes, at + 36, node.flags);
        word(&mut bytes, at + 40, 0);
        word(&mut bytes, at + 44, 1);
        word(&mut bytes, at + 48, node.zone_id);
        word(&mut bytes, at + 52, node.kind);
        word(
            &mut bytes,
            at + 56,
            node.data_ptr_override.unwrap_or(offsets[index]),
        );
        word(&mut bytes, at + 60, node.mesh_index as u32);
        word(&mut bytes, at + 64, 0);
        word(&mut bytes, at + 68, 1);
        word(&mut bytes, at + 72, 0);
        half(
            &mut bytes,
            at + 84,
            node.parent_count_override
                .unwrap_or(u16::from(node.parent.is_some())),
        );
        half(&mut bytes, at + 86, node.children.len() as u16);
        word(&mut bytes, at + 196, node.unk196);
        word(&mut bytes, at + 208, node.node_index);

        // The data record, at the offset the walk will reach.
        let mut at = offsets[index] as usize;
        match node.kind {
            5 => {
                let spec = node.object.expect("an object record has its fields");
                word(&mut bytes, at, spec.flags);
                for axis in 0..3 {
                    float(&mut bytes, at + 24 + 4 * axis, spec.rotation[axis]);
                    float(&mut bytes, at + 36 + 4 * axis, spec.scale[axis]);
                    float(&mut bytes, at + 84 + 4 * axis, spec.translation[axis]);
                    for column in 0..3 {
                        float(
                            &mut bytes,
                            at + 48 + 4 * (3 * axis + column),
                            spec.matrix[axis][column],
                        );
                    }
                }
                at += 144;
            }
            6 => {
                let spec = node.lod.expect("a LOD record has its fields");
                word(&mut bytes, at, spec.level);
                float(&mut bytes, at + 4, spec.range_near_sq);
                float(&mut bytes, at + 8, spec.range_far);
                float(&mut bytes, at + 12, spec.range_far_sq);
                float(&mut bytes, at + 64, spec.unk64);
                float(&mut bytes, at + 68, spec.unk64 * spec.unk64);
                float(&mut bytes, at + 72, spec.unk72);
                float(&mut bytes, at + 76, spec.unk72 * spec.unk72);
                word(&mut bytes, at + 80, 1);
                at += 92;
            }
            2 => {
                let spec = node.world.expect("a world record has its grid");
                word(&mut bytes, at + 152, spec.partition_x_count);
                word(&mut bytes, at + 156, spec.partition_y_count);
                word(&mut bytes, at + 176, spec.own_children_count);
                at += 208;
                // Each cell is 88 bytes followed by its own `count` × 12-byte
                // values, and both are laid out: the cell's `count` is what tells
                // the reader how many values follow it, so a cell written
                // without them would not be a record this layout describes.
                //
                // Cells are written for as many as the buffer really holds, so a
                // record whose stored counts are impossible is laid out honestly
                // at the length the caller chose and the counts are the only
                // thing wrong — which is what makes the refusal the grid's.
                let values_bytes = usize::from(spec.values_per_cell) * 12;
                let per_cell = 88 + values_bytes;
                let declared_cells = usize::try_from(spec.partition_x_count)
                    .expect("the fixture grid fits")
                    * usize::try_from(spec.partition_y_count).expect("the fixture grid fits");
                let mut room = node
                    .written_len()
                    .saturating_sub(at - offsets[index] as usize);
                let mut cell = 0;
                while cell < declared_cells {
                    // The cell's `count` is what tells the reader how many values
                    // follow it, so the header is written whenever the header
                    // itself fits — even when its values cannot. That is exactly
                    // the record a reader has to refuse; writing no `count` at
                    // all would hand it a grid that reads as empty instead.
                    if room < 88 {
                        break;
                    }
                    word(&mut bytes, at, 0x100);
                    half(&mut bytes, at + 58, spec.values_per_cell);
                    if room < per_cell {
                        // The declared `count` does not fit. The record is laid
                        // out honestly at the length the caller chose, and the
                        // count is the only thing wrong.
                        at += room;
                        break;
                    }
                    at += per_cell;
                    room -= per_cell;
                    cell += 1;
                }
            }
            // A tag outside the layout: the object record's length is written,
            // so the walk would be in step if the tag were accepted — which is
            // what makes the refusal the tag's own rather than the walk's.
            7 => {
                if let Some(spec) = node.object {
                    word(&mut bytes, at, spec.flags);
                }
                at += 144;
            }
            1 => at += 488,
            3 => at += 248,
            4 => at += 28,
            9 => at += 256,
            // Any other tag, the same: an undefined kind is written with the
            // object record's length so the refusal is the tag's own.
            _ => at += 144,
        }
        if node.parent_word_is_present() {
            // A light's word is written even when the record declares no
            // parent: the reference stores 0 there (the world node's index),
            // so that is what the writer stores too.
            word(&mut bytes, at, node.parent.unwrap_or(0));
            at += 4;
        }
        for (position, child) in node.children.iter().enumerate() {
            word(&mut bytes, at + 4 * position, *child);
        }
    }
    bytes
}

/// Reads one synthetic container through the production node reader.
fn read_fixture(label: &str, nodes: &[NodeSpec]) -> cs_formats::gamez::GameZNodes {
    let bytes = write_container(nodes);
    let mut context = cs_formats::ParseContext::with_defaults(label);
    cs_formats::gamez::read_gamez_nodes(&mut context, &bytes)
        .unwrap_or_else(|error| panic!("{label}: the fixture node array must read, got {error}"))
}

/// The mesh catalog the fixture's `mesh_index` values resolve against.
fn fixture_mesh_slots(count: usize) -> Vec<MeshSlot> {
    (0..count)
        .map(|slot| {
            MeshSlot::new(
                cid(ContentKind::Mesh, &format!("fixture.synthetic.s{slot}")),
                designed("t392.test.mesh-slot"),
            )
            .expect("a mesh slot in the mesh namespace")
        })
        .collect()
}

/// **The node array is two passes over two sections, and the reader proves it.**
///
/// A container whose node records hold distinct values in distinct slots has to
/// come back with every one of them in the right record, the info array's end
/// has to be the data section's start, and the data section's end has to be the
/// container's end. A reader that treated the array as one flat run of
/// fixed-size records, or that followed the stored `data_ptr` instead of
/// walking, could not hold all three.
#[test]
fn accept_t392_node_array_decodes_every_stored_field_into_its_own_slot() {
    // One airframe: a root, a wing carrying a mirrored tip, a gun on the wing,
    // and a LOD variant pair under the root.
    let nodes = vec![
        NodeSpec::new("main", 5)
            .object(ObjectSpec::transformed([0.0, 0.0, 0.0], [1.0, 2.0, 3.0]))
            .children(&[1, 4, 5]),
        NodeSpec::new("wing_l", 5)
            .parent(0)
            .children(&[2])
            .mesh(7)
            .zone(3)
            .object(ObjectSpec::transformed(
                [0.0, 0.0, std::f32::consts::FRAC_PI_2],
                [0.5, 0.0, 0.0],
            )),
        NodeSpec::new("gun", 5)
            .parent(1)
            .mesh(9)
            .object(ObjectSpec::transformed([0.1, 0.2, 0.3], [0.25, 0.0, -0.5])),
        // A record the layout stores with no transform at all.
        NodeSpec::new("tip", 5)
            .parent(1)
            .object(ObjectSpec::identity()),
        NodeSpec::new("wing_lod0", 6)
            .parent(0)
            .lod(lod(1, 100.0, 500.0)),
        NodeSpec::new("wing_lod1", 6)
            .parent(0)
            .lod(lod(0, 500.0, 2000.0)),
    ];
    let records = read_fixture("fixture.node-array", &nodes);

    // The header's own words gate the section, and the two passes tile it.
    assert_eq!(records.header.node_array_size, 6);
    assert_eq!(records.header.nodes_offset, FIXTURE_NODES_OFFSET);
    assert_eq!(records.nodes.len(), 6);
    assert_eq!(records.info_offset, u64::from(FIXTURE_NODES_OFFSET));
    assert_eq!(
        records.info_end,
        u64::from(FIXTURE_NODES_OFFSET) + 212 * 6,
        "the info array is 212 bytes per node"
    );
    assert_eq!(
        records.data_offset, records.info_end,
        "the data section starts exactly where the info array ends"
    );
    assert_eq!(
        records.data_end,
        write_container(&nodes).len() as u64,
        "the data section ends exactly at the container's end"
    );

    // Names, flags, zones and the trailing node index each land in their own
    // record; the trailing word crosses over whole.
    for (index, expected) in ["main", "wing_l", "gun", "tip", "wing_lod0", "wing_lod1"]
        .iter()
        .enumerate()
    {
        let node = records.get(index as u32).expect("every slot is present");
        assert_eq!(node.name, *expected, "node {index} keeps its authored name");
        assert_eq!(
            node.flags(),
            0x0180_0000,
            "node {index} keeps its raw flags"
        );
        assert_eq!(node.node_index, 0x0200_0000, "node {index} keeps its word");
        assert_eq!(node.engine_index(), 0, "the top byte is masked off");
    }
    assert_eq!(records.get(1).expect("wing").zone_id(), 3);
    assert_eq!(records.get(0).expect("root").zone_id(), 255);

    // The same two words cross over into the typed records unchanged: a zone id
    // and an uninterpreted flag word are F11-B's input, and a conversion that
    // substituted a constant for either would silently relabel the content.
    let typed =
        parsed_nodes_from_gamez(&records, &fixture_mesh_slots(16)).expect("the records convert");
    assert_eq!(
        typed.iter().map(|node| node.zone_id).collect::<Vec<_>>(),
        vec![255, 3, 255, 255, 255, 255],
        "every zone id crosses over exactly as stored"
    );
    assert!(
        typed.iter().all(|node| node.flags == 0x0180_0000),
        "every raw flag word crosses over uninterpreted"
    );

    // The hierarchy crosses over as stored slots, both directions.
    assert_eq!(records.get(0).expect("root").parent, None);
    assert_eq!(records.get(1).expect("wing").parent, Some(0));
    assert_eq!(records.get(0).expect("root").children, vec![1, 4, 5]);
    assert_eq!(records.get(1).expect("wing").children, vec![2]);
    assert_eq!(records.roots().count(), 1);

    // An object record's transform crosses over verbatim.
    let wing = records.get(1).expect("wing").object3d().expect("an object");
    assert_eq!(wing.flags, 32);
    assert_eq!(wing.translation, [0.5, 0.0, 0.0]);
    assert!(
        !wing.matrix_disagrees(),
        "the identity matrix matches no rotation"
    );
    let gun = records.get(2).expect("gun").object3d().expect("an object");
    assert_eq!(gun.rotation, [0.1, 0.2, 0.3]);
    assert_eq!(gun.translation, [0.25, 0.0, -0.5]);

    // A LOD record's near bound is stored squared; the far bound is stored
    // twice and the reader reports whether the two agree.
    let near = records.get(4).expect("lod0").lod().expect("a LOD record");
    assert_eq!(near.level, 1);
    assert_eq!(near.range_min(), Some(100.0));
    assert_eq!(near.range_far, 500.0);
    assert!(near.far_square_is_consistent());
    let far = records.get(5).expect("lod1").lod().expect("a LOD record");
    assert_eq!(far.level, 0);
    assert_eq!(far.range_min(), Some(500.0));
    assert_eq!(far.range_far, 2000.0);

    // The mesh association: `-1` is no binding, and a non-negative value is the
    // stored slot with its own id resolved from the caller's catalog.
    assert_eq!(records.get(0).expect("root").mesh_index(), -1);
    assert_eq!(records.get(1).expect("wing").mesh_index(), 7);
    assert_eq!(records.mesh_index_bounds().bound, 2);
    assert_eq!(records.mesh_index_bounds().min, Some(7));
    assert_eq!(records.mesh_index_bounds().max, Some(9));

    // The record's own bytes stay addressable, so a later stage re-derives the
    // unmeasured words from the same bytes the reader measured.
    let bytes = write_container(&nodes);
    for node in &records.nodes {
        let start = node.data_offset as usize;
        let end = start + node.data_bytes as usize;
        assert!(
            end <= bytes.len(),
            "node {} addresses real bytes",
            node.index
        );
        if node.index == 1 {
            assert_eq!(node.data_bytes, 144 + 4 + 4, "144 + parent + one child");
        }
    }
    assert!(
        records.findings.is_empty(),
        "a clean fixture is inside the profile"
    );
}

/// **The typed records carry the store's own data and nothing invented.**
#[test]
fn accept_t392_typed_records_keep_the_stored_transform_and_resolve_meshes() {
    // A record whose stored matrix disagrees with its own euler triple, next to
    // one that agrees, plus a node whose mesh slot the catalog cannot answer.
    // A 90°-shaped matrix stored against a zero euler triple: the two are
    // different transforms, which is exactly the case the reference's ~0.74 % of
    // records is.
    let disagreeing = ObjectSpec::disagreeing(
        [0.0, 0.0, 0.0],
        [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
        [4.0, 0.0, 0.0],
    );
    let nodes = vec![
        NodeSpec::new("root", 5)
            .object(ObjectSpec::transformed([0.0, 0.0, 0.0], [0.0; 3]))
            .children(&[1, 2, 3]),
        NodeSpec::new("stored_matrix", 5)
            .parent(0)
            .object(disagreeing),
        NodeSpec::new("euler_only", 5)
            .parent(0)
            .object(ObjectSpec::transformed(
                [0.0, 0.0, std::f32::consts::FRAC_PI_2],
                [1.0, 0.0, 0.0],
            )),
        NodeSpec::new("unresolved", 5)
            .parent(0)
            .mesh(11)
            .object(ObjectSpec::identity()),
    ];
    let records = read_fixture("fixture.typed", &nodes);
    // The disagreement is a finding, and the stored matrix is still what the
    // record holds.
    assert_eq!(
        records.findings.len(),
        1,
        "one record disagrees: {records:?}",
        records = records.findings
    );
    assert_eq!(
        records.findings[0],
        cs_formats::gamez::NodeFinding::ObjectMatrixDisagrees { node: 1 }
    );

    // The catalog answers slots 0..=9 and nothing else.
    let meshes = fixture_mesh_slots(10);
    let parsed = parsed_nodes_from_gamez(&records, &meshes).expect("the records convert");
    assert_eq!(parsed.len(), 4);

    // A disagreeing record keeps the stored matrix, because it is what the file
    // holds; an agreeing one keeps only the euler triple, because recomputing
    // over the stored matrix would hide that the two are the same transform.
    assert_eq!(
        parsed[1].transform.matrix,
        Some(disagreeing.matrix),
        "a stored matrix that disagrees wins over the euler triple"
    );
    assert_eq!(
        parsed[2].transform.matrix, None,
        "an agreeing stored matrix is not carried a second time"
    );
    assert_eq!(
        parsed[2].transform.rotation,
        [0.0, 0.0, std::f32::consts::FRAC_PI_2]
    );
    assert_eq!(parsed[2].transform.translation, [1.0, 0.0, 0.0]);

    // A record the store flagged as storing no transform carries the identity,
    // not four words that happen to be zero. The store holds exactly that, so
    // the two spellings agree here — what is asserted is that the conversion
    // took the flag's branch rather than reading the four zeroed words.
    assert_eq!(
        parsed[3].transform,
        AuthoredTransform::IDENTITY,
        "a flags == 40 record is the identity by the store's own rule"
    );
    assert_eq!(parsed[3].transform.scale, [1.0; 3]);
    assert_eq!(parsed[3].transform.matrix, None);

    // A mesh slot inside the catalog resolves with the catalog's own
    // provenance; one past it is an explicit unknown with a reason, never an
    // invented id.
    assert_eq!(parsed[0].mesh, None, "mesh_index -1 is no binding at all");
    let bound = parsed[3].mesh.as_ref().expect("a non-negative index binds");
    assert_eq!(bound.index, 11);
    match &bound.mesh {
        Resolved::Unknown { claim_id, reason } => {
            assert_eq!(claim_id, &claim("f11-node-array.mesh-slot-unresolved"));
            assert!(
                reason.contains("mesh index 11") && reason.contains("10 slot"),
                "the reason names the index and the slot count: {reason}"
            );
        }
        other => panic!("a slot past the catalog must stay unknown, got {other:?}"),
    }

    // The whole conversion still produced every record: one unresolvable slot is
    // not a refusal, because refusing would throw away an exact hierarchy over a
    // missing catalog row.
    assert_eq!(
        parsed.len(),
        4,
        "no record is dropped over a missing catalog row"
    );

    // A slot the catalog does answer resolves to its own element.
    let with_slot = vec![
        NodeSpec::new("bound", 5)
            .object(ObjectSpec::identity())
            .mesh(4),
    ];
    let bound_records = read_fixture("fixture.bound", &with_slot);
    let bound_parsed = parsed_nodes_from_gamez(&bound_records, &meshes).expect("one record");
    let binding = bound_parsed[0].mesh.as_ref().expect("a binding");
    assert_eq!(binding.index, 4);
    assert_eq!(
        match &binding.mesh {
            Resolved::Known(known) => Some(known.value.key().to_owned()),
            Resolved::Unknown { .. } => None,
        },
        Some("fixture.synthetic.s4".to_owned()),
        "the binding resolves to the catalog's element, not to a slot number"
    );
    assert_eq!(
        binding.mesh.provenance().map(|p| p.claim_id.clone()),
        Some(claim("t392.test.mesh-slot")),
        "the resolution carries the catalog's provenance"
    );
}

/// **A mesh slot the catalog cannot answer stays an explicit unknown.**
///
/// `MeshBinding` is defined to resolve an index to a catalog element *or* record
/// it unresolved, and a slot past the supplied catalog is exactly that second
/// case: the association is real and its index is carried, but nothing claims
/// which element it is.
#[test]
fn accept_t392_a_mesh_index_past_the_catalog_stays_an_explicit_unknown() {
    let nodes = vec![
        NodeSpec::new("root", 5)
            .object(ObjectSpec::identity())
            .mesh(3),
    ];
    let records = read_fixture("fixture.mesh-slot", &nodes);
    let parsed = parsed_nodes_from_gamez(&records, &fixture_mesh_slots(3))
        .expect("an unresolvable mesh slot is not a refusal of the record");
    let binding = parsed[0].mesh.as_ref().expect("the association is kept");
    assert_eq!(
        binding.index, 3,
        "the stored index is provenance, not dropped"
    );
    match &binding.mesh {
        Resolved::Unknown { claim_id, reason } => {
            assert_eq!(claim_id, &claim("f11-node-array.mesh-slot-unresolved"));
            assert!(
                reason.contains("mesh index 3") && reason.contains("3 slot"),
                "the reason names the index and the slot count: {reason}"
            );
        }
        other => panic!("a slot past the catalog must stay unknown, got {other:?}"),
    }

    // A catalog element in the wrong namespace is refused where it is declared.
    let error = MeshSlot::new(
        cid(ContentKind::Airframe, "fixture.synthetic.wrong"),
        designed("t392.test.mesh-slot"),
    )
    .expect_err("a catalog element that is not a mesh cannot be a mesh slot");
    assert!(matches!(
        error,
        GameZSceneError::MeshKind { kind, .. } if kind == ContentKind::Airframe
    ));
    // No node has used the slot yet, so the refusal names no node rather than
    // passing a sentinel a caller could mistake for a real array slot.
    assert_eq!(
        error.node(),
        None,
        "a construction refusal is not about a node"
    );
    assert!(
        error.to_string().contains("airframe"),
        "and it still says what the element really is: {error}"
    );
}

/// **The build sees the store's hierarchy, and its rejections stay typed.**
///
/// The records are produced whatever the hierarchy turns out to be; the
/// conversion's verdict is a separate step, and this is the F11-B shape the
/// earlier stages had to take on trust.
#[test]
fn accept_t392_scene_graph_is_built_from_a_decoded_node_array() {
    let nodes = vec![
        NodeSpec::new("corsair", 5)
            .object(ObjectSpec::transformed([0.0, 0.0, 0.0], [0.0, 0.0, 0.0]))
            .children(&[1]),
        // A 90° yaw with a translation: the stored euler triple becomes a
        // canonical local, and the child composes under it. The angle is in the
        // adapter's declared unit — the fixture source declares degrees — so the
        // stored value is 90, not π/2; that unit conversion is the F16-A
        // contract's and the composition is what this test is about.
        NodeSpec::new("wing_l", 5)
            .parent(0)
            .children(&[2])
            .mesh(2)
            .object(ObjectSpec::transformed(
                [0.0, 0.0, std::f32::consts::FRAC_PI_2],
                [3.0, 0.0, 0.0],
            )),
        NodeSpec::new("tip_l", 5)
            .parent(1)
            .object(ObjectSpec::transformed([0.0, 0.0, 0.0], [1.0, 0.0, 0.0])),
    ];
    let records = read_fixture("fixture.scene-graph", &nodes);
    let container = cid(ContentKind::SceneNode, "container.fixture");
    let meshes = fixture_mesh_slots(4);
    let graph = scene_graph_from_gamez(
        &container,
        &records,
        &meshes,
        &radian_adapter(),
        &BindingMap::default(),
    )
    .expect("a strict forest with usable names converts");

    assert_eq!(graph.len(), 3);
    assert_eq!(graph.container(), &container);
    assert_eq!(graph.roots().len(), 1);
    let root = graph.single_root().expect("one root");
    assert_eq!(root.name(), "corsair");
    // Identity derives a stable id from the container key and the authored
    // name-path, never from the array slot.
    assert_eq!(root.id().key(), "container.fixture.corsair");
    let wing = graph
        .node(
            &SceneNodeId::from_content_id(cid(
                ContentKind::SceneNode,
                "container.fixture.corsair.wing_l",
            ))
            .expect("a scene node id"),
        )
        .expect("the wing is in the graph");
    assert_eq!(
        wing.index(),
        1,
        "the array slot is provenance, not identity"
    );
    assert_eq!(
        wing.mesh().map(|binding| binding.index),
        Some(2),
        "the mesh association survives the conversion"
    );

    // The composed transform is the node's one pose, and the render and
    // collision paths see the same value (F11 behavior 4).
    assert_eq!(wing.visual_transform(), wing.collision_transform());
    // The wing's own authored translation crosses over, and the tip's has to
    // travel through the wing's yaw before the two are added, so a composition
    // that forgot the rotation — or applied it in the wrong order — lands
    // elsewhere. The reference's convention is `Rz·Ry·Rx` over **negated**
    // angles, so a 90° stored yaw carries a source +X offset onto source −Y.
    //
    // The tolerance here is f32's, not the suite's `EPSILON`: the store holds
    // `f32`, so the composed matrix carries `cos(π/2) ≈ -4.4e-8` and the exact
    // value is not reachable. That is the record's precision, not a slack in the
    // composition.
    const F32: f64 = 1e-6;
    let wing_world = wing.world_transform().translation();
    for axis in 0..3 {
        assert!(
            (wing_world[axis] - [3.0, 0.0, 0.0][axis]).abs() <= F32,
            "the wing's own translation: {wing_world:?}"
        );
    }
    let tip = graph
        .node(
            &SceneNodeId::from_content_id(cid(
                ContentKind::SceneNode,
                "container.fixture.corsair.wing_l.tip_l",
            ))
            .expect("a scene node id"),
        )
        .expect("the tip is in the graph");
    let composed = tip.world_transform().translation();
    for (axis, expected) in [3.0, -1.0, 0.0].into_iter().enumerate() {
        assert!(
            (composed[axis] - expected).abs() <= F32,
            "the tip composes under the wing's yaw: axis {axis}: {} != {expected}",
            composed[axis]
        );
    }
    assert!(!tip.mirrored(), "no authored negative scale here");

    // A LOD node crosses over with its resolved range, and the selection rule
    // runs over the converted graph.
    let lod_nodes = vec![
        NodeSpec::new("root", 5)
            .object(ObjectSpec::identity())
            .children(&[1, 2]),
        NodeSpec::new("band0", 6).parent(0).lod(lod(1, 0.0, 100.0)),
        NodeSpec::new("band1", 6)
            .parent(0)
            .lod(lod(0, 100.0, 1000.0)),
    ];
    let lod_records = read_fixture("fixture.lod-graph", &lod_nodes);
    let lod_graph = scene_graph_from_gamez(
        &cid(ContentKind::SceneNode, "container.fixture.lod"),
        &lod_records,
        &[],
        &radian_adapter(),
        &BindingMap::default(),
    )
    .expect("a LOD pair converts");
    let bands: Vec<LodInfo> = lod_graph
        .nodes()
        .iter()
        .filter_map(|node| node.lod().copied())
        .collect();
    assert_eq!(
        bands.len(),
        2,
        "both variants survive; nothing is flattened"
    );
    // `canonical` is one unit per metre, so the stored 0..100 stays 0..100 m.
    assert_eq!(bands[0].range_max, Meters(100.0));
    // The record stores the near bound **squared**, so a conversion that forgot
    // to take the root would produce a band boundary nine orders of magnitude
    // out — a distance no later stage could notice.
    assert_eq!(
        bands[0].range_min,
        Meters(0.0),
        "the stored near bound is resolved from its square"
    );
    assert_eq!(bands[1].range_min, Meters(100.0));
    assert_eq!(bands[1].range_max, Meters(1000.0));
    assert!(
        bands[0].level,
        "the stored level boolean crosses over as stored"
    );
    let choice = select_lod_variant(&bands, Meters(0.5)).expect("a usable distance");
    assert_eq!(choice.index, 0);
    assert_eq!(choice.coverage, LodCoverage::Covered);

    // A detached cycle is the build's refusal and arrives as its own variant,
    // with the records still produced. The cycle needs a root of its own to sit
    // beside: a forest with *only* a cycle has no root at all, which is the
    // separate `NoRoots` refusal, and both links in the cycle have to agree or
    // the inconsistent-link check fires first.
    let cyclic = vec![
        NodeSpec::new("loose_root", 5)
            .object(ObjectSpec::identity())
            .children(&[]),
        NodeSpec::new("a", 5)
            .parent(2)
            .children(&[2])
            .object(ObjectSpec::identity()),
        NodeSpec::new("b", 5)
            .parent(1)
            .children(&[1])
            .object(ObjectSpec::identity()),
    ];
    let cyclic_records = read_fixture("fixture.cycle", &cyclic);
    // The reader accepts it: the stored links are self-consistent, so the cycle
    // is a fact about the data rather than a decode failure.
    assert_eq!(cyclic_records.nodes.len(), 3);
    assert_eq!(cyclic_records.get(1).expect("a").parent, Some(2));
    assert_eq!(cyclic_records.get(1).expect("a").children, vec![2]);
    let error = scene_graph_from_gamez(
        &cid(ContentKind::SceneNode, "container.fixture.cycle"),
        &cyclic_records,
        &[],
        &radian_adapter(),
        &BindingMap::default(),
    )
    .expect_err("a detached two-node cycle is refused");
    assert_eq!(error.code(), "build");
    assert!(
        matches!(error, GameZSceneError::Build(SceneError::Cycle { .. })),
        "{error}"
    );

    // A forest with no root at all is refused as such, which is a different
    // condition from a cycle and says so.
    let rootless = vec![
        NodeSpec::new("a", 5)
            .parent(1)
            .children(&[1])
            .object(ObjectSpec::identity()),
        NodeSpec::new("b", 5)
            .parent(0)
            .children(&[0])
            .object(ObjectSpec::identity()),
    ];
    let rootless_records = read_fixture("fixture.rootless", &rootless);
    let error = scene_graph_from_gamez(
        &cid(ContentKind::SceneNode, "container.fixture.rootless"),
        &rootless_records,
        &[],
        &radian_adapter(),
        &BindingMap::default(),
    )
    .expect_err("a container with no root is refused");
    assert!(
        matches!(error, GameZSceneError::Build(SceneError::NoRoots)),
        "{error}"
    );

    // A name the id grammar refuses is reported as such and never transliterated.
    let awkward = vec![NodeSpec::new("brigturret2 ", 5).object(ObjectSpec::identity())];
    let awkward_records = read_fixture("fixture.awkward-name", &awkward);
    assert_eq!(
        awkward_records.get(0).expect("one node").name,
        "brigturret2 "
    );
    let error = scene_graph_from_gamez(
        &cid(ContentKind::SceneNode, "container.fixture"),
        &awkward_records,
        &[],
        &radian_adapter(),
        &BindingMap::default(),
    )
    .expect_err("a trailing space is not a key character");
    assert!(
        matches!(
            error,
            GameZSceneError::Build(SceneError::NodeId { node: 0, .. })
        ),
        "{error}"
    );
}

/// Two well-formed records, the smallest container that still has a data
/// section.
fn two_record_fixture() -> Vec<NodeSpec> {
    vec![
        NodeSpec::new("a", 5).object(ObjectSpec::identity()),
        NodeSpec::new("b", 5).object(ObjectSpec::identity()),
    ]
}

/// **Every byte-accounting failure is a typed refusal, never a wrong answer.**
#[test]
fn accept_t392_a_broken_node_array_is_refused_with_its_own_reason() {
    // A kind tag the CS layout does not define. The record carries the object
    // record's fields so the refusal has to come from the tag itself and not
    // from the walk.
    let bad_kind = vec![NodeSpec::new("odd", 7).object(ObjectSpec::identity())];
    let error = read_nodes_expecting_error("fixture.bad-kind", &bad_kind);
    assert!(
        matches!(
            error,
            cs_formats::gamez::GameZNodeError::NodeType {
                node: 0,
                found: 7,
                ..
            }
        ),
        "{error}"
    );

    // A record whose stored data pointer is not the offset the walk reaches.
    let moved = vec![
        NodeSpec::new("root", 5)
            .object(ObjectSpec::identity())
            .data_ptr(FIXTURE_NODES_OFFSET + 212 + 4),
    ];
    let error = read_nodes_expecting_error("fixture.moved-data", &moved);
    assert!(
        matches!(
            error,
            cs_formats::gamez::GameZNodeError::DataOffset { node: 0, .. }
        ),
        "{error}"
    );

    // A parent slot that names no node in the array. The refusal is anchored at
    // the node whose *record* carries the slot, not at the node it fails to
    // name, so the number in the report points at the bytes that are wrong.
    let bad_parent = vec![
        NodeSpec::new("root", 5)
            .object(ObjectSpec::identity())
            .children(&[1]),
        NodeSpec::new("child", 5)
            .parent(9)
            .object(ObjectSpec::identity()),
    ];
    let error = read_nodes_expecting_error("fixture.bad-parent", &bad_parent);
    assert!(
        matches!(
            error,
            cs_formats::gamez::GameZNodeError::ParentSlot {
                node: 1,
                found: 9,
                count: 2
            }
        ),
        "{error}"
    );

    // A child slot that names no node in the array.
    let bad_child = vec![
        NodeSpec::new("root", 5)
            .object(ObjectSpec::identity())
            .children(&[7]),
        NodeSpec::new("child", 5)
            .parent(0)
            .object(ObjectSpec::identity()),
    ];
    let error = read_nodes_expecting_error("fixture.bad-child", &bad_child);
    assert!(
        matches!(
            error,
            cs_formats::gamez::GameZNodeError::ChildSlot {
                node: 0,
                position: 0,
                found: 7,
                count: 2
            }
        ),
        "{error}"
    );

    // A header the container does not have: the signature is checked before the
    // node array is located, so a wrong one is a header refusal and never a
    // walk error.
    let two = two_record_fixture();
    let mut wrong_signature = write_container(&two);
    wrong_signature[0..4].copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
    let mut context = cs_formats::ParseContext::with_defaults("fixture.wrong-signature");
    let error = cs_formats::gamez::read_gamez_nodes(&mut context, &wrong_signature)
        .expect_err("a container that is not a GameZ container is refused");
    assert!(
        matches!(error, cs_formats::gamez::GameZNodeError::Header(_)),
        "{error}"
    );
    assert!(
        error.code() == "header",
        "the refusal says which condition it is: {error}"
    );

    // A `nodes_offset` that is not inside the container: the array cannot start in
    // the file at all, which is a different condition from an array that does
    // not fit once it has started. The shared header check already refuses an
    // offset *past* the end, so this variant's own case is an offset landing
    // exactly on the end — in bounds by the header's rule, with nothing left to
    // read.
    let mut moved_offset = write_container(&two);
    let at_the_end = u32::try_from(moved_offset.len()).expect("a fixture fits in u32");
    moved_offset[36..40].copy_from_slice(&at_the_end.to_le_bytes());
    let mut context = cs_formats::ParseContext::with_defaults("fixture.moved-nodes-offset");
    let error = cs_formats::gamez::read_gamez_nodes(&mut context, &moved_offset)
        .expect_err("a node array outside the container is refused");
    assert!(
        matches!(
            error,
            cs_formats::gamez::GameZNodeError::NodesOffsetOutOfBounds { .. }
        ),
        "{error}"
    );

    // One world cell whose own value count does not fit what is left of the data
    // section. The grid itself is one cell of 88 bytes, so the cell check passes
    // and it is the per-cell `count` that is impossible — a different way to
    // reach the same condition, and it must arrive as the grid's refusal rather
    // than as a bare truncation.
    let greedy_cell = vec![
        NodeSpec::new("world1", 2)
            .world(WorldSpec {
                partition_x_count: 1,
                partition_y_count: 1,
                values_per_cell: u16::MAX,
                own_children_count: 1,
            })
            .declared_len(208 + 88),
    ];
    let error = read_nodes_expecting_error("fixture.greedy-cell", &greedy_cell);
    match error {
        cs_formats::gamez::GameZNodeError::PartitionGrid { node, cells, .. } => {
            assert_eq!(node, 0);
            assert_eq!(cells, 1, "the grid itself fits: one cell");
        }
        other => panic!("an impossible per-cell count is a grid refusal, got {other}"),
    }

    // A world grid far larger than the data section that follows it. The record
    // is laid out with two cells, so the buffer is small and the stored counts
    // are the only thing wrong — which is what makes the refusal the grid's.
    let huge_world = vec![
        NodeSpec::new("world1", 2)
            .world(WorldSpec {
                partition_x_count: 4_000_000,
                partition_y_count: 4_000_000,
                values_per_cell: 0,
                own_children_count: 1,
            })
            .declared_len(208 + 2 * 88),
    ];
    let error = read_nodes_expecting_error("fixture.huge-world", &huge_world);
    match error {
        cs_formats::gamez::GameZNodeError::PartitionGrid {
            node,
            cells,
            available,
        } => {
            assert_eq!(node, 0);
            assert_eq!(cells, 4_000_000 * 4_000_000);
            assert!(
                available < 2 * 88 + 8,
                "the report says how little was left, got {available}"
            );
        }
        other => panic!("expected a partition grid refusal, got {other}"),
    }

    // A world grid whose cell counts overflow when multiplied is refused the
    // same way, with the saturating product in the report.
    let overflowing = vec![
        NodeSpec::new("world1", 2)
            .world(WorldSpec {
                partition_x_count: u32::MAX,
                partition_y_count: u32::MAX,
                values_per_cell: 0,
                own_children_count: 1,
            })
            .declared_len(208 + 88),
    ];
    let error = read_nodes_expecting_error("fixture.overflowing-world", &overflowing);
    assert!(
        matches!(
            error,
            cs_formats::gamez::GameZNodeError::PartitionGrid { cells, .. }
                if cells == u64::from(u32::MAX) * u64::from(u32::MAX)
        ),
        "{error}"
    );

    // A truncated info array: the second record's bytes are simply not there.
    // The header still declares both records, so the refusal is the *read*
    // running off the end — not the extent check, which passes because the
    // declared array does fit before the container's new end.
    let two = two_record_fixture();
    let mut truncated = write_container(&two);
    truncated.truncate(truncated.len() - 8);
    let mut context = cs_formats::ParseContext::with_defaults("fixture.truncated");
    let error = cs_formats::gamez::read_gamez_nodes(&mut context, &truncated)
        .expect_err("an info array that does not fit is refused");
    match error {
        cs_formats::gamez::GameZNodeError::Parse(ref error) => assert!(
            error.offset >= u64::from(FIXTURE_NODES_OFFSET) + 212,
            "the refusal points at the second record, past the first: {error}"
        ),
        other => panic!("a read past the end is a parse failure, got {other}"),
    }

    // Trailing bytes the data section does not account for: the walk ends early,
    // so the container does not end where the reader reached.
    let mut trailing = write_container(&two);
    trailing.extend_from_slice(&[0u8; 16]);
    let mut context = cs_formats::ParseContext::with_defaults("fixture.trailing");
    let error = cs_formats::gamez::read_gamez_nodes(&mut context, &trailing)
        .expect_err("a data section that does not reach the end is refused");
    assert!(
        matches!(error, cs_formats::gamez::GameZNodeError::DataEnd { .. }),
        "{error}"
    );

    // A name field with no terminator inside its own 36 bytes.
    let mut unterminated = write_container(&two);
    let base = FIXTURE_NODES_OFFSET as usize;
    unterminated[base..base + 36].fill(b'x');
    let mut context = cs_formats::ParseContext::with_defaults("fixture.unterminated");
    let error = cs_formats::gamez::read_gamez_nodes(&mut context, &unterminated)
        .expect_err("a name with no NUL inside its bound is refused");
    assert!(
        matches!(error, cs_formats::gamez::GameZNodeError::Parse(_)),
        "{error}"
    );

    // An empty node array: a container this layout does not describe, refused as
    // such rather than decoded into zero records. The header's own word says
    // zero and the body is left as padding, so nothing else is wrong.
    let mut empty = write_container(&two);
    empty[28..32].copy_from_slice(&0u32.to_le_bytes());
    empty.truncate(FIXTURE_NODES_OFFSET as usize + 16);
    let mut context = cs_formats::ParseContext::with_defaults("fixture.empty-array");
    let error = cs_formats::gamez::read_gamez_nodes(&mut context, &empty)
        .expect_err("a container declaring an empty node array is refused");
    assert!(
        matches!(error, cs_formats::gamez::GameZNodeError::NodeArrayEmpty),
        "{error}"
    );
}

/// Reads a fixture expecting the production reader to refuse it.
fn read_nodes_expecting_error(
    label: &str,
    nodes: &[NodeSpec],
) -> cs_formats::gamez::GameZNodeError {
    let bytes = write_container(nodes);
    let mut context = cs_formats::ParseContext::with_defaults(label);
    cs_formats::gamez::read_gamez_nodes(&mut context, &bytes)
        .expect_err("the fixture must be refused")
}

/// **The non-object kinds keep the walk in step, and the world's grid is sized
/// from its own counts.**
///
/// Six of the seven kinds are read for their length alone, and three of them
/// (window, camera, light) carry a parent word the object's record does not.
/// The walk is the only check on those sizes: a record read at the wrong length
/// leaves every later node mis-placed, and the container's end is where the
/// error surfaces. So this fixture holds one of every kind in a single container
/// — with a world grid that has cells and values, and a light node whose parent
/// word must be consumed although the reference asserts the boolean is clear for
/// it — and demands that the data section still end exactly at the container's
/// end.
#[test]
fn accept_t392_every_node_kind_keeps_the_data_walk_in_step() {
    let nodes = vec![
        // A world node: the 204-byte record, the one child-value word the
        // reference reads as a result of the record's own children count, and a
        // 2x2 grid whose cells carry values of their own.
        NodeSpec::new("world", 2)
            .world(WorldSpec {
                partition_x_count: 2,
                partition_y_count: 2,
                values_per_cell: 2,
                own_children_count: 1,
            })
            .children(&[1, 2]),
        NodeSpec::new("display", 4).parent(0),
        NodeSpec::new("window", 3).parent(0),
        // A camera with a child, and a light node that declares **no** parent.
        NodeSpec::new("camera", 1).parent(0).children(&[4]),
        // The light's stored `parent_count` is clear — the reference asserts it
        // is clear for every light — and its parent word is still in the
        // record. A reader that consulted the boolean here would stop 4 bytes
        // short and the container would not end where it was walked to.
        NodeSpec::new("light", 9),
    ];
    let bytes = write_container(&nodes);
    let mut context = cs_formats::ParseContext::with_defaults("fixture.every-kind");
    let records = cs_formats::gamez::read_gamez_nodes(&mut context, &bytes)
        .expect("one container holding every kind must read");

    assert_eq!(
        records.data_end,
        bytes.len() as u64,
        "five kinds with three different parent-word rules and a variable world \
         block still land on the container's last byte"
    );
    assert_eq!(
        records.findings.is_empty(),
        records.nodes.iter().all(|node| node.info.parent_count <= 1),
        "a fixture inside the asserted profile raises no finding: {:?}",
        records.findings
    );

    // The world's grid is what makes its variable-length block knowable: four
    // cells of 88 bytes, each followed by its own two 12-byte values.
    let world = records.get(0).expect("world").kind;
    let cs_formats::gamez::NodeKind::World(world) = world else {
        panic!("node 0 is a world record")
    };
    assert_eq!(world.partition_x_count, 2);
    assert_eq!(world.partition_y_count, 2);
    assert_eq!(world.partition_values, 8, "four cells, two values each");
    assert_eq!(
        world.partition_bytes,
        4 * (88 + 2 * 12),
        "the block is the cells and their values together"
    );

    // A light node's parent word is read even though the record declares no
    // parent, and it is not exposed as a parent link. Both halves matter: the
    // word has to be consumed for the walk to stay in step, and reporting it as
    // a parent would invent a hierarchy the record does not declare.
    let light = records.get(4).expect("light");
    assert!(
        matches!(light.kind, cs_formats::gamez::NodeKind::Light),
        "node 4 is a light record, got {:?}",
        light.kind
    );
    assert_eq!(
        light.info.parent_count, 0,
        "this fixture clears the boolean the reference says is clear for a light"
    );
    assert_eq!(
        light.parent, None,
        "a word the record does not declare as a parent is not exposed as one"
    );
    // Its 256-byte record plus the parent word is exactly its data extent.
    assert_eq!(
        light.data_bytes,
        256 + 4,
        "the 256-byte record and the unconditional parent word"
    );

    // Every other kind's own bytes stay addressable, which is how a later stage
    // reads the camera's FOV or the window's flags without this reader having to
    // have guessed them.
    assert_eq!(records.get(1).expect("display").data_bytes, 28 + 4);
    assert_eq!(records.get(2).expect("window").data_bytes, 248 + 4);
    assert_eq!(records.get(3).expect("camera").data_bytes, 488 + 4 + 4);
}

/// **A node kind the layout does not define is refused, whatever its bytes.**
///
/// The tag is checked while the info array is being read, so a container whose
/// second record carries an unknown kind must not be reported as a truncation or
/// a walk error: the refusal names the node and the word it read.
#[test]
fn accept_t392_an_unknown_node_kind_is_refused_by_its_tag() {
    for tag in [cs_formats::gamez::NODE_TYPE_EMPTY, 7, 8, 10, 0xFFFF_FFFF] {
        let nodes = vec![
            NodeSpec::new("good", 5).object(ObjectSpec::identity()),
            NodeSpec::new("odd", tag).object(ObjectSpec::identity()),
        ];
        let error = read_nodes_expecting_error("fixture.unknown-kind", &nodes);
        match error {
            cs_formats::gamez::GameZNodeError::NodeType {
                node,
                offset,
                found,
            } => {
                assert_eq!(node, 1, "the second record is the odd one");
                assert_eq!(found, tag);
                assert_eq!(
                    offset,
                    // The `node_type` word sits at offset 52 of the 212-byte slot:
                    // the 36-byte name field, then the eleven words before it.
                    u64::from(FIXTURE_NODES_OFFSET) + 212 + cs_formats::gamez::NODE_TYPE_OFFSET,
                    "the refusal is anchored at the node_type word it read"
                );
            }
            other => panic!("tag {tag} must be refused as an unknown kind, got {other}"),
        }
    }
}

/// **A record outside the reference's asserted profile is read and reported.**
#[test]
fn accept_t392_records_outside_the_asserted_profile_are_reported_not_dropped() {
    let mut bad_flags = ObjectSpec::transformed([0.0; 3], [0.0; 3]);
    bad_flags.flags = 7;
    let mut identity_but_not = ObjectSpec::identity();
    identity_but_not.translation = [1.0, 0.0, 0.0];
    let nodes = vec![
        NodeSpec::new("root", 5)
            .object(ObjectSpec::identity())
            .children(&[1, 2, 3, 4, 5, 6, 7, 8, 9]),
        NodeSpec::new("bad_flags", 5).parent(0).object(bad_flags),
        NodeSpec::new("identity_but_not", 5)
            .parent(0)
            .object(identity_but_not),
        // A LOD record whose stored far square is not its far bound squared.
        NodeSpec::new("far_square", 6).parent(0).lod(LodSpec {
            range_far_sq: 7.0,
            ..lod(1, 10.0, 20.0)
        }),
        // A LOD record whose near bound is stored as a negative square.
        NodeSpec::new("near_negative", 6).parent(0).lod(LodSpec {
            range_near_sq: -4.0,
            ..lod(1, 2.0, 20.0)
        }),
        NodeSpec::new("level_three", 6)
            .parent(0)
            .lod(lod(3, 1.0, 2.0)),
        NodeSpec::new("field196", 5)
            .parent(0)
            .field196(0)
            .object(ObjectSpec::identity()),
        NodeSpec::new("parent_count", 5)
            .parent(0)
            .parent_count(4)
            .object(ObjectSpec::identity()),
        NodeSpec::new("mesh_sentinel", 5)
            .parent(0)
            .mesh(-7)
            .object(ObjectSpec::identity()),
        NodeSpec::new("node_index", 5)
            .parent(0)
            .node_index(0x0100_0007)
            .object(ObjectSpec::identity()),
        // A world record whose **own** children count is not the one the
        // reference asserts. It is a different field from the info record's
        // `children_count`, which is what the child slots follow, so nothing
        // about the walk depends on it and the record is still read whole.
        NodeSpec::new("world", 2).parent(0).world(WorldSpec {
            partition_x_count: 1,
            partition_y_count: 1,
            values_per_cell: 0,
            own_children_count: 2,
        }),
    ];
    let records = read_fixture("fixture.findings", &nodes);

    // Every record is still read: a finding is not a refusal.
    assert_eq!(records.nodes.len(), 11);
    assert_eq!(
        records
            .get(3)
            .expect("lod")
            .lod()
            .expect("lod")
            .range_far_sq,
        7.0
    );
    assert_eq!(records.get(0).expect("root").name, "root");
    // The world's own children count is not the field the walk uses, so the
    // record reads whole and its grid is still the one its counts declare.
    let cs_formats::gamez::NodeKind::World(world) = records.get(10).expect("world").kind else {
        panic!("node 10 is a world record")
    };
    assert_eq!(world.partition_x_count, 1);
    assert_eq!(world.partition_bytes, 88);

    let codes: Vec<(&str, u32)> = records
        .findings
        .iter()
        .map(|finding| (finding.code(), finding.node()))
        .collect();
    for expected in [
        ("object_flags", 1),
        ("object_identity_not_identity", 2),
        ("lod_far_square", 3),
        ("lod_near_square_negative", 4),
        ("lod_level", 5),
        ("node_field_196", 6),
        ("parent_count", 7),
        ("mesh_index_sentinel", 8),
        ("node_index_top_bits", 9),
        ("world_children_count", 10),
    ] {
        assert!(
            codes.contains(&expected),
            "expected {expected:?} among {codes:?}"
        );
    }

    // A record the store flags as holding no transform but which stores a
    // translation anyway keeps its own words. The reader has already reported
    // the disagreement; discarding the numbers here would erase the only trace
    // of it before a caller could see it.
    let parsed = parsed_nodes_from_gamez(
        &read_fixture(
            "fixture.flagged-not-identity",
            &[NodeSpec::new("flagged", 5).object(identity_but_not)],
        ),
        &[],
    )
    .expect("a flagged record that stores a translation is not a refusal");
    assert_eq!(
        parsed[0].transform.translation,
        [1.0, 0.0, 0.0],
        "the store's own words survive the conversion"
    );
    assert_ne!(
        parsed[0].transform,
        AuthoredTransform::IDENTITY,
        "and the record is not silently relabelled as the identity"
    );

    // A negative near bound has no real root, so the typed conversion refuses
    // that one record rather than producing a NaN distance.
    let error = parsed_nodes_from_gamez(&records, &[]).expect_err("a negative near bound");
    assert_eq!(
        error,
        GameZSceneError::LodNearBound {
            node: 4,
            found: -4.0
        }
    );
}

/// **The retail half: the layout holds on the original installation's bytes.**
///
/// This is the evidence that the layout is not a guess. It reads the real
/// `planes.zbd` through the production reader, checks the numbers the pinned
/// reference records for that archive, and then reports the *conversion's*
/// verdict on it honestly — including the refusal, which is a fact about the
/// data meeting F11-A's id scheme and not about this reader.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t392_retail_planes_node_array_decodes_and_its_conversion_verdict_is_typed() {
    use cs_formats::gamez::NodeFinding;

    let path = retail_dir().join("ZBD/planes.zbd");
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|error| panic!("zbd/planes.zbd: the installation must hold it: {error}"));
    let mut context = cs_formats::ParseContext::with_defaults("zbd/planes.zbd");
    let records = cs_formats::gamez::read_gamez_nodes(&mut context, &bytes)
        .expect("the retail node array must read");

    // The header words are the pinned reference's own recorded numbers.
    assert_eq!(records.header.node_array_size, 3_317);
    assert_eq!(records.header.nodes_offset, 4_881_228);
    assert_eq!(records.header.light_index, 2_338);
    // The two passes tile the container exactly: this is the check that no
    // record was skipped and none was read at the wrong length.
    assert_eq!(records.info_offset, 4_881_228);
    assert_eq!(records.info_end, 4_881_228 + 212 * 3_317);
    assert_eq!(records.data_offset, records.info_end);
    assert_eq!(
        records.data_end,
        bytes.len() as u64,
        "the data section ends exactly at the container's end"
    );
    assert_eq!(records.nodes.len(), 3_317);

    // `planes.zbd` holds only object and LOD nodes: it is the shared aircraft
    // geometry container, not a world.
    let mut objects = 0;
    let mut lods = 0;
    for node in &records.nodes {
        match node.kind {
            cs_formats::gamez::NodeKind::Object3d(_) => objects += 1,
            cs_formats::gamez::NodeKind::Lod(_) => lods += 1,
            other => panic!("planes.zbd holds no {} node", other.label()),
        }
    }
    assert_eq!((objects, lods), (3_230, 87));

    // The stored hierarchy is a strict forest: every link agrees in both
    // directions, and every node is reachable from a root.
    assert_eq!(records.roots().count(), 28);
    let mut linked = 0usize;
    for node in &records.nodes {
        for child in &node.children {
            assert_eq!(
                records.get(*child).and_then(|c| c.parent),
                Some(node.index),
                "node {} lists child {child}, which does not name it back",
                node.index
            );
            linked += 1;
        }
    }
    assert_eq!(linked, 3_289, "every non-root node is listed exactly once");

    // The mesh association names slots inside the container's mesh array.
    let bounds = records.mesh_index_bounds();
    assert_eq!(bounds.bound, 1_766);
    assert_eq!(bounds.min, Some(0));
    assert_eq!(bounds.max, Some(1_778));

    // The measured corpus is inside the reference's asserted profile apart from
    // the stored-matrix disagreements the reference itself documents.
    let disagreements: Vec<u32> = records
        .findings
        .iter()
        .filter_map(|finding| match finding {
            NodeFinding::ObjectMatrixDisagrees { node } => Some(*node),
            _ => None,
        })
        .collect();
    assert_eq!(disagreements.len(), 107, "the measured disagreement count");
    for finding in &records.findings {
        assert_eq!(
            finding.code(),
            "object_matrix_disagrees",
            "no other deviation: {finding}"
        );
    }

    // The typed records convert: 3 317 of them, with the same slots and links.
    let container = cid(ContentKind::SceneNode, "container.zbd.planes");
    let meshes: Vec<MeshSlot> = (0..=bounds.max.expect("planes names meshes"))
        .map(|slot| {
            MeshSlot::new(
                cid(ContentKind::Mesh, &format!("container.zbd.planes.s{slot}")),
                designed("t392.retail.mesh-slot"),
            )
            .expect("a mesh id")
        })
        .collect();
    let parsed = parsed_nodes_from_gamez(&records, &meshes).expect("the records convert");
    assert_eq!(parsed.len(), 3_317);
    assert_eq!(parsed[0].index, 0);
    assert_eq!(parsed[0].name, "wf2test");
    assert_eq!(parsed[0].kind, ParsedNodeKind::Object3d);
    // `wf2test` is one of the records the layout stores with no transform.
    assert_eq!(parsed[0].transform, AuthoredTransform::IDENTITY);
    let bound = parsed.iter().filter_map(|node| node.mesh.as_ref()).count();
    assert_eq!(bound, 1_766, "one binding per non-negative mesh_index");

    // The conversion's verdict on the real container is a typed refusal, and it
    // is the authored name that causes it: `brigturret2 ` stores a trailing
    // space, which the id grammar does not allow. This is a fact about the data
    // meeting F11-A's id scheme, recorded rather than worked around.
    let error = scene_graph_from_gamez(
        &container,
        &records,
        &meshes,
        &radian_adapter(),
        &BindingMap::default(),
    )
    .expect_err("the stored name with a trailing space is not a key character");
    let GameZSceneError::Build(SceneError::NodeId { node, source }) = &error else {
        panic!("expected a node id refusal, got {error}");
    };
    assert_eq!(*node, 640, "the first node the id grammar refuses");
    assert!(
        source.to_string().contains('\''),
        "the refusal names the offending character: {source}"
    );
    // Exactly six nodes carry that name or descend from it; the whole container
    // is otherwise a forest of usable name-paths.
    let mut refused = 0usize;
    for node in &parsed {
        let mut path = String::new();
        let mut cursor = Some(node.index);
        while let Some(index) = cursor {
            let current = &parsed[index as usize];
            if path.is_empty() {
                path = current.name.clone();
            } else {
                path = format!("{path}.{}", current.name);
            }
            cursor = current.parent;
        }
        if ContentId::from_source(
            ContentKind::SceneNode,
            &format!("container.zbd.planes.{path}"),
        )
        .is_err()
        {
            refused += 1;
        }
    }
    assert_eq!(
        refused, 6,
        "the six nodes whose name-path carries the space: 640 and its five descendants"
    );
}

/// Every GameZ archive of the installation, with the numbers the layout's
/// central claim rests on: the stored record count, the offset the array starts
/// at, and how many records store a matrix their own euler triple does not
/// derive.
///
/// The spelling is written out rather than derived from the logical key,
/// because this test reads files and no discovery decides their spelling. The
/// `nodes_offset` column is not repeated here: [`RETAIL_GAMEZ_NODES_OFFSET`]
/// already pins it against the pinned reference, and this test checks that the
/// array *starts* on the header's own word.
const RETAIL_NODE_ARRAYS: [(&str, u32, u32); 9] = [
    ("ZBD/planes.zbd", 3_317, 107),
    ("ZBD/C1/gamez.zbd", 7_064, 5),
    ("ZBD/C1B/gamez.zbd", 5_603, 1),
    ("ZBD/C1C/gamez.zbd", 5_644, 1),
    ("ZBD/C2/gamez.zbd", 4_956, 1),
    ("ZBD/C2B/gamez.zbd", 4_901, 1),
    ("ZBD/C3/gamez.zbd", 5_408, 18),
    ("ZBD/C4/gamez.zbd", 8_289, 11),
    ("ZBD/C5/gamez.zbd", 11_438, 3),
];

/// **The layout holds on every GameZ archive of the installation, not one.**
///
/// The claim the whole worksheet rests on is that the data section is
/// variable-length and ends **exactly** at each container's end. That is
/// falsifiable per container and only nine containers exist, so it is pinned
/// here for all nine: each one reads, each one's info array starts on the
/// header's own `nodes_offset`, and each one's walk lands on its container's
/// last byte. A record read at the wrong length would leave every later node
/// mis-placed and stop short, so this is the test that would catch it.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t392_retail_every_gamez_archive_walks_to_its_container_end() {
    use cs_formats::gamez::{NodeFinding, NodeKind};

    let mut total_nodes = 0u64;
    let mut kinds: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut total_disagreements = 0u64;
    for (relative, expected_nodes, expected_disagreements) in RETAIL_NODE_ARRAYS {
        let path = retail_dir().join(relative);
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|error| panic!("{relative}: the installation must hold it: {error}"));
        let mut context = cs_formats::ParseContext::with_defaults(relative);
        let records = cs_formats::gamez::read_gamez_nodes(&mut context, &bytes)
            .unwrap_or_else(|error| panic!("{relative}: the retail node array must read: {error}"));

        assert_eq!(records.header.node_array_size, expected_nodes, "{relative}");
        assert_eq!(records.nodes.len() as u32, expected_nodes, "{relative}");
        assert_eq!(
            records.info_offset,
            u64::from(records.header.nodes_offset),
            "{relative}: the info array starts on the header's own word"
        );
        assert_eq!(
            records.data_offset,
            records.header.nodes_offset as u64 + 212 * u64::from(expected_nodes),
            "{relative}: 212 bytes per slot, info record then node index word"
        );
        assert_eq!(records.data_offset, records.info_end, "{relative}");
        assert_eq!(
            records.data_end,
            bytes.len() as u64,
            "{relative}: the data section ends exactly at the container's end"
        );

        let disagreements = records
            .findings
            .iter()
            .filter(|finding| matches!(finding, NodeFinding::ObjectMatrixDisagrees { .. }))
            .count() as u64;
        assert_eq!(
            disagreements,
            u64::from(expected_disagreements),
            "{relative}: the stored-matrix disagreement count"
        );
        for finding in &records.findings {
            assert_eq!(
                finding.code(),
                "object_matrix_disagrees",
                "{relative}: no other deviation from the reference's profile: {finding}"
            );
        }

        total_nodes += records.nodes.len() as u64;
        total_disagreements += disagreements;
        for node in &records.nodes {
            let label = match node.kind {
                NodeKind::World(_) => "world",
                NodeKind::Camera => "camera",
                NodeKind::Window => "window",
                NodeKind::Display => "display",
                NodeKind::Light => "light",
                NodeKind::Object3d(_) => "object3d",
                NodeKind::Lod(_) => "lod",
            };
            *kinds.entry(label).or_default() += 1;
        }
    }

    // The corpus totals, as measured: nine containers, 56 620 stored records,
    // and 148 of them storing a matrix their own euler triple does not derive.
    assert_eq!(total_nodes, 56_620);
    assert_eq!(total_disagreements, 148);
    // Eight of the nine containers are world containers, so the six singleton
    // kinds occur eight times each — two windows and two cameras per world
    // container, one display and one light. `planes.zbd` holds neither.
    assert_eq!(
        kinds,
        BTreeMap::from([
            ("object3d", 51_611),
            ("lod", 4_953),
            ("world", 8),
            ("window", 16),
            ("camera", 16),
            ("display", 8),
            ("light", 8),
        ]),
        "every kind the CS layout defines occurs, and the split is the measured one"
    );
}
