//! `accept_f17_a_` tests for the golden synthetic scene and the ordered
//! draw plan: `cs_app::render::golden` + `cs_app::render::plan`.

use cs_app::render::golden::golden_scene;
use cs_app::render::material::{
    Classification, Coverage, DeclaredClass, MaterialClass, MaterialFacts, RenderPhase, classify,
};
use cs_app::render::plan::{DrawItem, DrawItemKey, DrawPlan, SceneView, SortingLimitation};
use cs_formats::texture::{AlphaSource, AlphaTest};
use cs_types::evidence::{ClaimStatus, ContentHash};

fn material(
    class: MaterialClass,
    coverage: Coverage,
    alpha_test: AlphaTest,
) -> cs_app::render::material::ClassifiedMaterial {
    let facts = MaterialFacts {
        declared: Some(DeclaredClass::new(class, ClaimStatus::Designed).expect("Designed asserts")),
        coverage,
        alpha_test,
        two_sided: Some(false),
        addressing: None,
        vertex_colors: false,
        unknown_flag_bits: 0,
    };
    match classify(&facts) {
        Classification::Classified(material) => material,
        Classification::Unclassified { reasons } => panic!("fixture material: {reasons:?}"),
    }
}

fn item(
    key: &str,
    class: MaterialClass,
    coverage: Coverage,
    alpha_test: AlphaTest,
    center_m: [f32; 3],
) -> DrawItem {
    DrawItem::new(
        DrawItemKey::new(key).expect("test keys are valid"),
        material(class, coverage, alpha_test),
        center_m,
        None,
    )
    .expect("finite test geometry")
}

fn plan_order(plan: &DrawPlan) -> Vec<(&str, RenderPhase)> {
    plan.entries()
        .iter()
        .map(|entry| (entry.key.as_str(), entry.phase))
        .collect()
}

/// AC01: the golden synthetic scene holds overlapping glass, an alpha-cut
/// fence, an additive sprite and per-corner colors, and the plan orders
/// them into the fixed phases — opaque and masked in submission order,
/// translucent back-to-front, additive after all translucency even though
/// the sprite sits between the panes in depth.
#[test]
fn accept_f17_a_golden_scene_orders_glass_fence_sprite_and_corner_colors() {
    let scene = golden_scene();

    // The fixture is scrambled on purpose: a pre-sorted list would not
    // exercise the plan.
    let submitted: Vec<&str> = scene
        .items()
        .iter()
        .map(|item| item.key().as_str())
        .collect();
    assert_eq!(
        submitted,
        [
            "sprite",
            "glass_near",
            "percorner",
            "fence",
            "glass_far",
            "ground"
        ]
    );

    let plan = scene.draw_plan();
    assert_eq!(
        plan_order(&plan),
        [
            ("percorner", RenderPhase::Opaque),
            ("ground", RenderPhase::Opaque),
            ("fence", RenderPhase::Masked),
            ("glass_far", RenderPhase::Translucent),
            ("glass_near", RenderPhase::Translucent),
            ("sprite", RenderPhase::Additive),
        ]
    );
    assert!(plan.limitations().is_empty());

    // Every submitted item draws exactly once.
    let mut drawn: Vec<usize> = plan.entries().iter().map(|entry| entry.item).collect();
    drawn.sort_unstable();
    assert_eq!(drawn, (0..scene.items().len()).collect::<Vec<_>>());

    // The sprite's depth (5.6 m) sits between glass_far (6.2 m) and
    // glass_near (5.0 m), yet additive still draws after both panes:
    // phase order, not depth, decides across phases.
    let depth = |key: &str| {
        plan.entries()
            .iter()
            .find(|entry| entry.key.as_str() == key)
            .expect("every item is planned")
            .depth_m
    };
    assert!(depth("glass_far") > depth("sprite") && depth("sprite") > depth("glass_near"));

    // Per-corner colors travel bit-exact: the authored four corners.
    let percorner = scene.item("percorner").expect("the scene has the quad");
    assert_eq!(
        percorner.corner_colors(),
        Some([
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
        ])
    );
    assert!(percorner.material().vertex_colors());
    assert!(
        percorner
            .material()
            .unknowns()
            .contains(&cs_app::render::material::MaterialUnknown::VertexColorMeaning)
    );

    // The fence is masked at its authored threshold; the glass is blended.
    let fence = scene.item("fence").expect("the scene has the fence");
    assert_eq!(fence.material().class(), MaterialClass::Masked);
    assert_eq!(fence.material().alpha_test(), AlphaTest::Threshold(0x80));
    assert_eq!(
        scene
            .item("glass_near")
            .expect("glass")
            .material()
            .coverage(),
        Coverage::Texture(AlphaSource::Channel)
    );

    // The fixture's status travels: authored design, never measured
    // original behavior.
    for item in scene.items() {
        assert_eq!(item.material().status(), ClaimStatus::Designed);
    }
}

/// Translucency sorts by view depth, not submission order: submitting the
/// near pane first must still draw the far pane first.
#[test]
fn accept_f17_a_translucency_sorts_back_to_front() {
    let view = SceneView::new([0.0, 0.0, 4.0], [0.0, 0.0, -1.0]).expect("valid view");
    for items in [
        vec![
            item(
                "near",
                MaterialClass::Blended,
                Coverage::Uniform(100),
                AlphaTest::Disabled,
                [0.0, 0.0, -1.0],
            ),
            item(
                "far",
                MaterialClass::Blended,
                Coverage::Uniform(100),
                AlphaTest::Disabled,
                [0.0, 0.0, -2.0],
            ),
        ],
        vec![
            item(
                "far",
                MaterialClass::Blended,
                Coverage::Uniform(100),
                AlphaTest::Disabled,
                [0.0, 0.0, -2.0],
            ),
            item(
                "near",
                MaterialClass::Blended,
                Coverage::Uniform(100),
                AlphaTest::Disabled,
                [0.0, 0.0, -1.0],
            ),
        ],
    ] {
        let plan = DrawPlan::build(&items, &view);
        assert_eq!(
            plan_order(&plan),
            [
                ("far", RenderPhase::Translucent),
                ("near", RenderPhase::Translucent)
            ]
        );
        assert!(plan.limitations().is_empty());
    }

    // Moving the view flips the order: the contract is depth against the
    // one view, not a fixed item order.
    let from_behind = SceneView::new([0.0, 0.0, -4.0], [0.0, 0.0, 1.0]).expect("valid view");
    let items = [
        item(
            "near",
            MaterialClass::Blended,
            Coverage::Uniform(100),
            AlphaTest::Disabled,
            [0.0, 0.0, -1.0],
        ),
        item(
            "far",
            MaterialClass::Blended,
            Coverage::Uniform(100),
            AlphaTest::Disabled,
            [0.0, 0.0, -2.0],
        ),
    ];
    let plan = DrawPlan::build(&items, &from_behind);
    assert_eq!(
        plan_order(&plan),
        [
            ("near", RenderPhase::Translucent),
            ("far", RenderPhase::Translucent)
        ]
    );
}

/// Two translucent surfaces at the same view depth have no correct
/// object-level order: both still draw, the deterministic submission-order
/// tie-break applies, and the limitation is reported (spec F17
/// non-negotiable #2) rather than a pane hidden.
#[test]
fn accept_f17_a_equal_depth_translucency_is_reported_not_hidden() {
    let view = SceneView::new([0.0, 0.0, 4.0], [0.0, 0.0, -1.0]).expect("valid view");
    let items = [
        item(
            "pane_a",
            MaterialClass::Blended,
            Coverage::Uniform(100),
            AlphaTest::Disabled,
            [-1.0, 0.0, -2.0],
        ),
        item(
            "pane_b",
            MaterialClass::Blended,
            Coverage::Uniform(100),
            AlphaTest::Disabled,
            [1.0, 0.0, -2.0],
        ),
        item(
            "solid",
            MaterialClass::Opaque,
            Coverage::Opaque,
            AlphaTest::Disabled,
            [0.0, 0.0, -3.0],
        ),
    ];
    let plan = DrawPlan::build(&items, &view);
    assert_eq!(
        plan_order(&plan),
        [
            ("solid", RenderPhase::Opaque),
            ("pane_a", RenderPhase::Translucent),
            ("pane_b", RenderPhase::Translucent),
        ]
    );
    assert_eq!(
        plan.limitations(),
        [SortingLimitation::EqualViewDepth {
            phase: RenderPhase::Translucent,
            first: DrawItemKey::new("pane_a").expect("valid"),
            second: DrawItemKey::new("pane_b").expect("valid"),
        }]
    );
}

/// Opaque and masked items keep authored submission order inside their
/// phases: authored material ordering is preserved, never re-sorted by
/// depth (spec F17 non-negotiable #1).
#[test]
fn accept_f17_a_opaque_and_masked_keep_authored_order() {
    let view = SceneView::new([0.0, 0.0, 4.0], [0.0, 0.0, -1.0]).expect("valid view");
    // Submitted nearest-first on purpose: depth must not reorder these
    // phases, only authored order applies.
    let items = [
        item(
            "near_solid",
            MaterialClass::Opaque,
            Coverage::Opaque,
            AlphaTest::Disabled,
            [0.0, 0.0, -1.0],
        ),
        item(
            "far_solid",
            MaterialClass::Opaque,
            Coverage::Opaque,
            AlphaTest::Disabled,
            [0.0, 0.0, -3.0],
        ),
        item(
            "fence",
            MaterialClass::Masked,
            Coverage::Texture(AlphaSource::Channel),
            AlphaTest::Threshold(0x80),
            [0.0, 0.0, -2.0],
        ),
    ];
    let plan = DrawPlan::build(&items, &view);
    assert_eq!(
        plan_order(&plan),
        [
            ("near_solid", RenderPhase::Opaque),
            ("far_solid", RenderPhase::Opaque),
            ("fence", RenderPhase::Masked),
        ]
    );
}

/// The golden plan's fingerprint is pinned: phase, key and depth of every
/// entry, in draw order. Any classification or ordering change must move
/// it — and must be a deliberate, reviewable change, not a silent drift.
#[test]
fn accept_f17_a_golden_plan_fingerprint_is_pinned() {
    let scene = golden_scene();
    let plan = scene.draw_plan();
    assert_eq!(
        plan.fingerprint(),
        ContentHash::from_hex("413f2b94e6cbdd0c436a61677c2135526b473053892214076725284690d31235")
            .expect("hex"),
        "the golden plan changed; update the pin only with a reviewed change"
    );
}

/// The authored scene's fingerprint is pinned too, so the fixture's own
/// contents (items, classes, coverage, colors, view) cannot drift
/// silently.
#[test]
fn accept_f17_a_golden_scene_fingerprint_is_pinned() {
    let scene = golden_scene();
    assert_eq!(
        scene.fingerprint(),
        ContentHash::from_hex("5ec2d447f61a51f6d5c10568d9757771ad480bb7f920211c06b00d7b0d04b41f")
            .expect("hex"),
        "the golden scene changed; update the pin only with a reviewed change"
    );
}

/// Invalid inputs are refused at construction: a non-finite center or
/// color, a zero view direction, a key outside the grammar.
#[test]
fn accept_f17_a_invalid_inputs_are_refused() {
    let opaque = material(MaterialClass::Opaque, Coverage::Opaque, AlphaTest::Disabled);
    assert!(
        DrawItem::new(
            DrawItemKey::new("nan").expect("valid"),
            opaque.clone(),
            [f32::NAN, 0.0, 0.0],
            None,
        )
        .is_err()
    );
    assert!(
        DrawItem::new(
            DrawItemKey::new("nan_color").expect("valid"),
            opaque,
            [0.0, 0.0, 0.0],
            Some([
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0],
                [f32::INFINITY, 0.0, 0.0],
            ]),
        )
        .is_err()
    );
    assert!(SceneView::new([0.0, 0.0, 0.0], [0.0, 0.0, 0.0]).is_err());
    assert!(SceneView::new([f32::NAN, 0.0, 0.0], [0.0, 0.0, -1.0]).is_err());
    assert!(DrawItemKey::new("").is_err());
    assert!(DrawItemKey::new("White Space").is_err());
    assert!(DrawItemKey::new(&"k".repeat(65)).is_err());
}
