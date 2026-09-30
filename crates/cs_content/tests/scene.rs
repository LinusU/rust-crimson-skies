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
