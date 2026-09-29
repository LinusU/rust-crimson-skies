//! The golden synthetic render-test scene of stage `### F17-A`
//! (`specs/F17-rendering-material-fidelity-and-scalable-presentation.md`,
//! AC01).
//!
//! [`golden_scene`] builds the minimum discriminating scene the sheet
//! pins: **overlapping glass** (two blended panes at different view
//! depths), an **alpha-cut fence** (masked), an **additive sprite**
//! (between the panes in depth, but drawn after both), a **per-corner
//! colored** quad and an opaque ground. Every material carries a
//! [`DeclaredClass`] with [`ClaimStatus::Designed`]: the fixture is
//! authored engineering content, and that status travels with the
//! classified materials so no stage can later mistake it for measured
//! original behavior.
//!
//! The items are submitted in a deliberately scrambled order, so the
//! scene only reads correctly if [`DrawPlan`] does its job — the fixture
//! is an ordering test, not a pre-sorted list.

use cs_assets::install::sha256;
use cs_formats::texture::{AlphaSource, AlphaTest};
use cs_types::evidence::{ClaimStatus, ContentHash};

use crate::render::material::{
    AddressMode, Classification, ClassifiedMaterial, Coverage, DeclaredClass, MaterialClass,
    MaterialFacts, TextureAddress, classify,
};
use crate::render::plan::{DrawItem, DrawItemKey, DrawPlan, SceneView};

/// One item's place in the golden scene.
struct ItemSpec {
    key: &'static str,
    class: MaterialClass,
    coverage: Coverage,
    alpha_test: AlphaTest,
    two_sided: bool,
    center_m: [f32; 3],
    corner_colors: Option<[[f32; 3]; 4]>,
}

const REPEAT: TextureAddress = TextureAddress {
    u: AddressMode::Repeat,
    v: AddressMode::Repeat,
};

const CLAMP: TextureAddress = TextureAddress {
    u: AddressMode::Clamp,
    v: AddressMode::Clamp,
};

/// The authored contents of the golden scene.
///
/// The view sits at `(0, 0, 4)` looking toward `-Z`; `center_m` gives each
/// item a distinct view depth so the translucent sort is exercised:
/// ground 10 m, fence 7 m, `glass_far` 6.2 m, sprite 5.6 m,
/// `glass_near` 5.0 m, `percorner` 4.6 m. The submission order is
/// scrambled on purpose.
const ITEMS: &[ItemSpec] = &[
    ItemSpec {
        key: "sprite",
        class: MaterialClass::Additive,
        coverage: Coverage::Opaque,
        alpha_test: AlphaTest::Disabled,
        two_sided: false,
        center_m: [0.3, 0.0, -1.6],
        corner_colors: None,
    },
    ItemSpec {
        key: "glass_near",
        class: MaterialClass::Blended,
        coverage: Coverage::Texture(AlphaSource::Channel),
        alpha_test: AlphaTest::Disabled,
        two_sided: true,
        center_m: [0.4, 0.0, -1.0],
        corner_colors: None,
    },
    ItemSpec {
        key: "percorner",
        class: MaterialClass::Opaque,
        coverage: Coverage::Opaque,
        alpha_test: AlphaTest::Disabled,
        two_sided: false,
        center_m: [0.0, 0.0, -0.6],
        corner_colors: Some([
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
        ]),
    },
    ItemSpec {
        key: "fence",
        class: MaterialClass::Masked,
        coverage: Coverage::Texture(AlphaSource::Channel),
        alpha_test: AlphaTest::Threshold(0x80),
        two_sided: true,
        center_m: [0.0, 0.0, -3.0],
        corner_colors: None,
    },
    ItemSpec {
        key: "glass_far",
        class: MaterialClass::Blended,
        coverage: Coverage::Uniform(102),
        alpha_test: AlphaTest::Disabled,
        two_sided: true,
        center_m: [-0.4, 0.0, -2.2],
        corner_colors: None,
    },
    ItemSpec {
        key: "ground",
        class: MaterialClass::Opaque,
        coverage: Coverage::Opaque,
        alpha_test: AlphaTest::Disabled,
        two_sided: false,
        center_m: [0.0, -0.5, -6.0],
        corner_colors: None,
    },
];

/// The built golden scene: validated items plus the view the plan sorts
/// against.
///
/// Construction is infallible *by construction*: every spec is a constant
/// the compiler ships through `classify`, and `build` panics if an
/// authored item ever stops classifying — a broken fixture must fail
/// loudly at scene build, not degrade into a different scene.
pub struct GoldenScene {
    items: Vec<DrawItem>,
    view: SceneView,
}

/// Builds the golden scene: classifies every authored spec and assembles
/// the item list in the fixed submission order.
pub fn golden_scene() -> GoldenScene {
    let view = SceneView::new([0.0, 0.0, 4.0], [0.0, 0.0, -1.0])
        .expect("the authored view is finite with a nonzero forward");
    let items = ITEMS
        .iter()
        .map(|spec| {
            let material = classify_spec(spec);
            DrawItem::new(
                DrawItemKey::new(spec.key).expect("authored keys are valid"),
                material,
                spec.center_m,
                spec.corner_colors,
            )
            .expect("authored geometry is finite")
        })
        .collect();
    GoldenScene { items, view }
}

fn classify_spec(spec: &ItemSpec) -> ClassifiedMaterial {
    let declared =
        DeclaredClass::new(spec.class, ClaimStatus::Designed).expect("Designed asserts a class");
    let facts = MaterialFacts {
        declared: Some(declared),
        coverage: spec.coverage,
        alpha_test: spec.alpha_test,
        two_sided: Some(spec.two_sided),
        addressing: Some(if spec.class == MaterialClass::Blended {
            CLAMP
        } else {
            REPEAT
        }),
        vertex_colors: spec.corner_colors.is_some(),
        unknown_flag_bits: 0,
    };
    match classify(&facts) {
        Classification::Classified(material) => material,
        Classification::Unclassified { reasons } => {
            panic!(
                "authored item {:?} stopped classifying: {reasons:?}",
                spec.key
            )
        }
    }
}

impl GoldenScene {
    /// The scene's items in submission order.
    pub fn items(&self) -> &[DrawItem] {
        &self.items
    }

    /// The view the plan sorts against.
    pub const fn view(&self) -> &SceneView {
        &self.view
    }

    /// The ordered draw plan for this scene.
    pub fn draw_plan(&self) -> DrawPlan {
        DrawPlan::build(&self.items, &self.view)
    }

    /// The item with this key.
    pub fn item(&self, key: &str) -> Option<&DrawItem> {
        self.items.iter().find(|item| item.key().as_str() == key)
    }

    /// A canonical fingerprint of the authored scene: the view and every
    /// item's key, class, coverage, alpha test, two-sidedness, addressing,
    /// center and corner colors, in submission order.
    ///
    /// This identifies the fixture itself (an artifact), so a test can pin
    /// the golden input as well as the ordered plan.
    pub fn fingerprint(&self) -> ContentHash {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"cs/render/golden/v1\0");
        for value in self.view.position_m() {
            bytes.extend_from_slice(&value.to_bits().to_le_bytes());
        }
        for value in self.view.forward() {
            bytes.extend_from_slice(&value.to_bits().to_le_bytes());
        }
        bytes.extend_from_slice(&(self.items.len() as u32).to_le_bytes());
        for item in &self.items {
            bytes.extend_from_slice(item.key().as_str().as_bytes());
            bytes.push(0);
            let material = item.material();
            bytes.extend_from_slice(material.class().code().as_bytes());
            bytes.push(0);
            match material.coverage() {
                Coverage::Opaque => bytes.push(0),
                Coverage::Unknown => bytes.push(1),
                Coverage::Uniform(alpha) => {
                    bytes.push(2);
                    bytes.push(alpha);
                }
                Coverage::Texture(source) => {
                    bytes.push(3);
                    bytes.extend_from_slice(format!("{source:?}").as_bytes());
                }
            }
            match material.alpha_test() {
                AlphaTest::Disabled => bytes.push(0),
                AlphaTest::Unknown => bytes.push(1),
                AlphaTest::Threshold(t) => {
                    bytes.push(2);
                    bytes.push(t);
                }
            }
            bytes.push(u8::from(material.two_sided().unwrap_or(false)));
            bytes.push(u8::from(material.two_sided().is_some()));
            if let Some(address) = material.addressing() {
                bytes.extend_from_slice(address.u.code().as_bytes());
                bytes.push(b':');
                bytes.extend_from_slice(address.v.code().as_bytes());
            }
            bytes.push(0);
            for value in item.center_m() {
                bytes.extend_from_slice(&value.to_bits().to_le_bytes());
            }
            match item.corner_colors() {
                None => bytes.push(0),
                Some(corners) => {
                    bytes.push(1);
                    for corner in corners {
                        for channel in corner {
                            bytes.extend_from_slice(&channel.to_bits().to_le_bytes());
                        }
                    }
                }
            }
        }
        sha256(&bytes)
    }
}
