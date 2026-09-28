//! Acceptance contracts F04-A: what an [`AssetKey`] is, what a
//! [`ResolveContext`] carries, how precedence is labeled and how a
//! [`SourceSpan`] records an origin.
//!
//! These tests exercise production code only — `cs_types::asset_id`,
//! the contract half of spec F04. Removing or loosening that
//! implementation (accepting `..` in a key, folding the namespace, ranking
//! precedence as `verified_original`, dropping the checked range) makes
//! them fail.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use cs_types::asset_id::{
    AssetKey, AssetKeyError, AssetVariant, ContextError, LabelError, MAX_LABEL_LEN, MissionScope,
    ModId, ModStack, MountId, MountNamespace, PRECEDENCE_ORDER_STATUS, PrecedenceClass,
    ResolveContext, SourceSpan, SourceSpanError, WorldGroup,
};
use cs_types::evidence::{ClaimStatus, ContentHash};
use cs_types::install::{LocaleLabel, RelativePathError};

/// The synthetic fixture digest used wherever a context needs an
/// installation fingerprint. Newly authored fixture data, never an
/// original installation's hash.
fn fixture_install() -> ContentHash {
    ContentHash::from_bytes([0x11; 32])
}

fn namespace() -> MountNamespace {
    MountNamespace::new("content").expect("fixture namespace is valid")
}

fn digest(byte: u8) -> ContentHash {
    ContentHash::from_bytes([byte; 32])
}

/// The key is (mount namespace, logical path, variant), and the logical
/// path keeps the caller's spelling while the lookup form is normalized.
#[test]
fn accept_f04_a_asset_key_is_namespace_path_variant() {
    let key = AssetKey::from_spelling("content", "Textures\\HUD\\Alert.DDS", "default")
        .expect("fixture key is valid");

    assert_eq!(key.namespace().as_str(), "content");
    assert_eq!(*key.namespace(), namespace(), "the fixture namespace");
    assert_eq!(key.path().as_str(), "Textures\\HUD\\Alert.DDS");
    assert_eq!(key.variant().as_str(), "default");
    assert_eq!(
        key.path_key(),
        "textures/hud/alert.dds",
        "the lookup form folds case and separators"
    );
    assert_eq!(key.logical_key(), "content/default/textures/hud/alert.dds");
    assert_eq!(
        key.to_string(),
        "content/default/Textures\\HUD\\Alert.DDS",
        "the displayed key quotes the original spelling"
    );
}

/// Spec F04 non-negotiable behavior 1: normalize separators and ASCII
/// case for lookup, retain the original spelling.
#[test]
fn accept_f04_a_asset_key_lookup_is_logical_but_keeps_spelling() {
    let backslash = AssetKey::from_spelling("content", "Textures\\HUD\\Alert.dds", "default")
        .expect("fixture key is valid");
    let forward = AssetKey::from_spelling("content", "textures/hud/alert.dds", "default")
        .expect("fixture key is valid");

    assert_eq!(backslash, forward, "one legacy path, two spellings");
    assert_eq!(
        hash_of(&backslash),
        hash_of(&forward),
        "equal keys hash equally, so a lookup map cannot split them"
    );
    assert_eq!(
        backslash.cmp(&forward),
        std::cmp::Ordering::Equal,
        "equal keys order equal"
    );
    assert_eq!(backslash.logical_key(), forward.logical_key());
    assert_ne!(
        backslash.path().as_str(),
        forward.path().as_str(),
        "the original spelling is still there after normalization"
    );

    let other_variant = AssetKey::from_spelling("content", "textures/hud/alert.dds", "large")
        .expect("fixture key is valid");
    let other_path = AssetKey::from_spelling("content", "textures/hud/hud.dds", "default")
        .expect("fixture key is valid");
    let other_namespace = AssetKey::from_spelling("sound", "textures/hud/alert.dds", "default")
        .expect("fixture key is valid");
    assert_ne!(backslash, other_variant, "the variant is part of the key");
    assert_ne!(backslash, other_path, "the path is part of the key");
    assert_ne!(
        backslash, other_namespace,
        "the mount namespace is part of the key"
    );
}

/// Spec F04 non-negotiable behavior 1: a key can never be built from a
/// spelling that could leave a mount root.
#[test]
fn accept_f04_a_asset_key_rejects_escaping_spellings() {
    let rejected: [(&str, RelativePathError); 11] = [
        ("", RelativePathError::Empty),
        ("..", RelativePathError::ParentComponent),
        ("..\\escape.dds", RelativePathError::ParentComponent),
        ("../escape.dds", RelativePathError::ParentComponent),
        ("/absolute.dds", RelativePathError::Absolute),
        ("\\absolute.dds", RelativePathError::Absolute),
        ("C:\\windows\\alert.dds", RelativePathError::Absolute),
        ("c:/windows/alert.dds", RelativePathError::Absolute),
        ("a//b.dds", RelativePathError::EmptyComponent),
        ("hud/", RelativePathError::EmptyComponent),
        ("hud/./alert.dds", RelativePathError::CurrentComponent),
    ];
    for (path, reason) in rejected {
        let err = AssetKey::from_spelling("content", path, "default")
            .expect_err("an escaping spelling must not become a key");
        assert_eq!(
            err,
            AssetKeyError::Path(reason),
            "spelling {path:?} is rejected for the right reason"
        );
        assert!(
            err.to_string().contains("asset key path is invalid"),
            "the rejection names the path: {err}"
        );
    }

    let with_nul = AssetKey::from_spelling("content", "hud\0alert.dds", "default")
        .expect_err("a NUL byte must not become a key");
    assert_eq!(
        with_nul,
        AssetKeyError::Path(RelativePathError::InteriorNul)
    );

    assert_eq!(
        AssetKey::from_spelling("Content", "hud/alert.dds", "default"),
        Err(AssetKeyError::Namespace(LabelError::BadFirst {
            label: "mount namespace",
            ch: 'C',
        })),
        "a namespace is validated too"
    );
    assert_eq!(
        AssetKey::from_spelling("content", "hud/alert.dds", "Large"),
        Err(AssetKeyError::Variant(LabelError::BadFirst {
            label: "asset variant",
            ch: 'L',
        })),
        "a variant is validated too"
    );
}

/// The label vocabulary is open — no exhaustive set is claimed — but it is
/// validated, and every rejection names the field that failed.
#[test]
fn accept_f04_a_labels_validate_an_open_vocabulary() {
    assert_eq!(
        MountNamespace::new("zbd.texture")
            .map(|ns| ns.to_string())
            .map_err(|e| e.to_string()),
        Ok("zbd.texture".to_owned()),
        "labels stay open: nothing here claims an exhaustive namespace set"
    );
    assert_eq!(
        AssetVariant::default().as_str(),
        "default",
        "the neutral variant is the engine's authored label"
    );

    assert_eq!(
        MountNamespace::new(""),
        Err(LabelError::Empty {
            label: "mount namespace"
        })
    );
    assert_eq!(
        MountNamespace::new("Textures"),
        Err(LabelError::BadFirst {
            label: "mount namespace",
            ch: 'T'
        })
    );
    assert_eq!(
        MountNamespace::new("has/slash"),
        Err(LabelError::BadCharacter {
            label: "mount namespace",
            ch: '/'
        }),
        "a label may not contain a separator, so a logical key stays injective"
    );
    assert_eq!(
        MountNamespace::new(&"a".repeat(MAX_LABEL_LEN + 1)),
        Err(LabelError::TooLong {
            label: "mount namespace",
            len: MAX_LABEL_LEN + 1
        })
    );
    assert!(
        MountNamespace::new(&"a".repeat(MAX_LABEL_LEN)).is_ok(),
        "the documented maximum is inclusive"
    );

    for err in [
        AssetVariant::new("").expect_err("empty variant"),
        ModId::new("has space").expect_err("mod id with a space"),
        MissionScope::new("Mission 1").expect_err("mission scope with a space"),
        MountId::new("WORLD.C1").expect_err("uppercase mount id"),
    ] {
        let rendered = err.to_string();
        assert!(
            rendered.contains("label must not be empty")
                || rendered.contains("disallowed character")
                || rendered.contains("must start with a lowercase"),
            "the rejection names the rule: {rendered}"
        );
    }

    assert_eq!(
        MountId::new("world.c1")
            .map(|id| id.to_string())
            .map_err(|e| e.to_string()),
        Ok("world.c1".to_owned())
    );
    assert_eq!(
        MissionScope::new("m01")
            .map(|scope| scope.to_string())
            .map_err(|e| e.to_string()),
        Ok("m01".to_owned())
    );
    assert_eq!(
        ModId::new("hd-pack")
            .map(|id| id.to_string())
            .map_err(|e| e.to_string()),
        Ok("hd-pack".to_owned())
    );
}

/// Spec F04 non-negotiable behavior 2 fixes the order; the baseline order
/// is labeled **designed** until F04-D measures original lookup behavior.
#[test]
fn accept_f04_a_precedence_is_ranked_and_labeled_designed() {
    assert!(
        PrecedenceClass::Mod.rank() > PrecedenceClass::Patch.rank(),
        "opt-in mods outrank patch overlays"
    );
    assert!(
        PrecedenceClass::Patch.rank() > PrecedenceClass::MissionWorld.rank(),
        "patch overlays outrank mission/world-specific sources"
    );
    assert!(
        PrecedenceClass::MissionWorld.rank() > PrecedenceClass::Shared.rank(),
        "mission/world-specific sources outrank shared sources"
    );

    assert_eq!(
        PRECEDENCE_ORDER_STATUS,
        ClaimStatus::Designed,
        "the measured original ordering does not exist yet, so the baseline \
         order must say `designed`"
    );
    assert_ne!(
        PRECEDENCE_ORDER_STATUS,
        ClaimStatus::VerifiedOriginal,
        "nothing here may be presented as verified original behavior"
    );

    assert_eq!(PrecedenceClass::Mod.label(), "mod");
    assert_eq!(PrecedenceClass::Patch.label(), "patch");
    assert_eq!(PrecedenceClass::MissionWorld.label(), "mission_world");
    assert_eq!(PrecedenceClass::Shared.label(), "shared");
}

/// The context carries the five fields the sheet names, and a mod may not
/// be opted into twice (which would make mod precedence ambiguous).
#[test]
fn accept_f04_a_resolve_context_carries_its_scope() {
    let world = WorldGroup::new("zbd/c1").expect("fixture world group is valid");
    let context = ResolveContext::new(fixture_install())
        .with_world_group(world.clone())
        .with_locale(LocaleLabel::new("english").expect("fixture locale is valid"))
        .with_mission(MissionScope::new("m01").expect("fixture mission is valid"))
        .with_mods(
            ModStack::new(vec![ModId::new("hd-pack").expect("fixture mod is valid")])
                .expect("a single mod is not a duplicate"),
        );

    assert_eq!(context.installation, fixture_install());
    assert_eq!(context.world_group, Some(world));
    assert_eq!(
        context.locale.as_ref().map(|locale| locale.as_str()),
        Some("english")
    );
    assert_eq!(
        context.mission.as_ref().map(MissionScope::as_str),
        Some("m01")
    );
    assert_eq!(context.mods.len(), 1);
    assert!(!context.mods.is_empty());

    let empty = ResolveContext::new(fixture_install());
    assert_eq!(empty.world_group, None);
    assert_eq!(empty.locale, None);
    assert_eq!(empty.mission, None);
    assert!(empty.mods.is_empty());
    assert!(ModStack::empty().as_slice().is_empty());

    let hd = ModId::new("hd-pack").expect("fixture mod is valid");
    let hq = ModId::new("hq-pack").expect("fixture mod is valid");
    let stack = ModStack::new(vec![hd.clone(), hq.clone()]).expect("two mods are distinct");
    assert_eq!(stack.position(&hd), Some(0), "load order is preserved");
    assert_eq!(
        stack.position(&hq),
        Some(1),
        "later mods outrank earlier ones"
    );
    assert!(stack.contains(&hd));
    assert!(!stack.contains(&ModId::new("other").expect("label is valid")));

    let duplicate = ModStack::new(vec![hd.clone(), hq.clone(), hd.clone()])
        .expect_err("a repeated mod must be refused");
    assert_eq!(
        duplicate,
        ContextError::DuplicateMod {
            id: hd,
            first: 0,
            second: 2,
        }
    );
}

/// `IDENTITY-CONTENT`: a span names its installation, container and
/// member, and its range is an unsigned checked range.
#[test]
fn accept_f04_a_source_span_records_an_immutable_checked_origin() {
    let span = SourceSpan::new(
        fixture_install(),
        "zbd/c1/planes.zbd",
        Some("Textures\\HUD\\Alert.dds"),
        4096,
        128,
        Some(digest(0x5a)),
    )
    .expect("fixture span is valid");

    assert_eq!(span.install_sha256(), fixture_install());
    assert_eq!(span.container_path(), "zbd/c1/planes.zbd");
    assert_eq!(span.member_key(), Some("Textures\\HUD\\Alert.dds"));
    assert_eq!(span.offset(), 4096);
    assert_eq!(span.length(), 128);
    assert_eq!(span.member_sha256(), Some(digest(0x5a)));
    assert_eq!(
        span.byte_span(),
        cs_types::evidence::SourceSpan {
            offset: 4096,
            length: 128
        },
        "the contract span converts to the observation byte span"
    );
    assert!(
        span.to_string().contains("zbd/c1/planes.zbd"),
        "the span quotes its container: {span}"
    );

    assert_eq!(
        SourceSpan::new(fixture_install(), "zbd/planes.zbd", None, u64::MAX, 1, None),
        Err(SourceSpanError::RangeOverflow {
            offset: u64::MAX,
            length: 1
        }),
        "an out-of-range span is refused, never wrapped"
    );
    assert_eq!(
        SourceSpan::new(fixture_install(), "", None, 0, 8, None),
        Err(SourceSpanError::EmptyContainer)
    );
    assert_eq!(
        SourceSpan::new(fixture_install(), "zbd/planes.zbd", Some(""), 0, 8, None),
        Err(SourceSpanError::EmptyMemberKey)
    );
    assert!(
        SourceSpan::new(fixture_install(), "zbd/planes.zbd", None, u64::MAX, 0, None).is_ok(),
        "the checked range is offset + length, so an empty tail at the very \
         top of the range is still a range"
    );
}

fn hash_of(key: &AssetKey) -> u64 {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish()
}
