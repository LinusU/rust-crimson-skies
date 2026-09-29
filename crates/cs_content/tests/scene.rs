//! Acceptance scenario F11-A (AC01): nested transforms and negative scale
//! preserve visual/collision alignment after canonical conversion — plus the
//! hierarchy-validation failure cases and the semantic binding records.
//!
//! These tests exercise production code only: `cs_content::scene` over the
//! declared `cs_content::coordinates` adapters and the `cs_types` identity
//! records. Removing or neutering the axis-map conjugation, the composition
//! order, the mirror tracking or the link validation makes them fail.
//!
//! All fixture values are newly authored; nothing reads original data.

use cs_content::coordinates::SourceAdapter;
use cs_content::scene::{
    AnimationBinding, AuthoredTransform, BindingMap, CollisionRole, MeshBinding, NodeKind,
    ParsedNode, ParsedNodeKind, PartRole, SceneError, SceneGraph, SceneNodeId, SceneRootRef,
    SemanticBinding,
};
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;

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
