//! Acceptance scenario F11-A (AC01): nested transforms and negative scale
//! preserve visual/collision alignment after canonical conversion — plus the
//! hierarchy-validation failure cases and the semantic binding records; and
//! the F11-B (AC02) LOD selection rule over converted bands.
//!
//! These tests exercise production code only: `cs_content::scene` over the
//! declared `cs_content::coordinates` adapters and the `cs_types` identity
//! records. Removing or neutering the axis-map conjugation, the composition
//! order, the mirror tracking, the link validation or the LOD band rule
//! makes them fail.
//!
//! All fixture values are newly authored; nothing reads original data.

use cs_content::coordinates::SourceAdapter;
use cs_content::scene::{
    AirframeBlocker, AirframeRoster, AnimationBinding, AuditGap, AuthoredTransform, BindingMap,
    CollisionRole, ContainerBlocker, ContainerOutcome, ForcedMissionAssignment, LodChoice,
    LodCoverage, LodInfo, LodSelectError, MeshBinding, NodeKind, ParsedNode, ParsedNodeKind,
    PartRole, RosterAvailability, RosterEntry, RosterError, SceneContainerRef, SceneError,
    SceneGraph, SceneNodeId, SceneRootRef, SemanticBinding, select_lod_variant,
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
/// records it per container, but no independent copy of those numbers exists
/// in this repository, so the retail test only asserts the ones it can check
/// two ways (the mesh reader and the material reader reading the same 40
/// header bytes) and that the count is non-zero.
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
/// container, so no filename decides this; the second production reader of the
/// same 40 header bytes must agree about the node array, or the census would
/// be measuring a disagreement. Rows come back in logical-key order.
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
        assert_eq!(
            (
                materials.header.node_array_size,
                materials.header.nodes_offset
            ),
            (meshes.header.node_array_size, meshes.header.nodes_offset),
            "{logical}: the two readers disagree about the node array"
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

/// AC04 over the real installation. Every GameZ archive the owner has is
/// discovered by production discovery, classified by the production GameZ
/// reader, and measured: how many stored node records each container's own
/// header declares and where the array starts, cross-checked against the
/// pinned reference and against the second production reader of the same 40
/// header bytes.
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
    assert!(
        report.is_empty() || !report.is_complete(),
        "an audit that mapped nothing is never a pass"
    );
    assert!(!report.is_complete());
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
     Resolving task: the follow-up filed with this stage for roster discovery, which needs a \
     decoded node array (#392) before a name can be bound to a root.",
    "Roster availability is a designed vocabulary with no measured original meaning: which modes \
     let a player choose which airframe has not been observed, and a model name is still not \
     proof. Affected content: the selectable roster in every mode. Resolving tasks: the F22/F49 \
     mode and preset stages together with the roster-discovery follow-up.",
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
