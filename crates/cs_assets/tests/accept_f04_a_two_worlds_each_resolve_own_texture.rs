//! Acceptance scenario F04-A (AC01): two worlds contain a same-named
//! texture with different hashes, and each resolves its own version — plus
//! the precedence, ambiguity, scope and rejection cases around it.
//!
//! These tests exercise production code only: `cs_assets::vfs` (the
//! `MountBuilder`/`Vfs`/`resolve` path) over `cs_types::asset_id`. Every
//! mount below is *fixture data* — authored bytes in memory, never original
//! content — so the assertions are about resolution behavior, not about
//! any retail archive.
//!
//! Removing or neutering the implementation makes them fail: ignoring the
//! world group returns world `zbd/c2`'s digest for `zbd/c1`; ranking by
//! registration order picks the first mounted duplicate instead of failing
//! with both origins; flattening members into a first-wins map turns the
//! ambiguity case into a silent success.

use cs_assets::vfs::{
    AttemptOutcome, LookupOrder, LookupOrderStatus, MountBuilder, MountError, ResolveError,
    SkipReason, Vfs,
};
use cs_types::asset_id::{
    AssetKey, AssetVariant, MissionScope, ModId, ModStack, MountId, MountNamespace,
    PrecedenceClass, ResolveContext, WorldGroup,
};
use cs_types::evidence::{ClaimStatus, ContentHash};
use cs_types::install::{LocaleLabel, RelativePathError};

/// The single mount namespace the fixture serves. Engine-authored label;
/// the retail namespace vocabulary is unknown (spec F04 research boundary).
const NAMESPACE: &str = "content";

/// The installation fingerprint every fixture context resolves against.
/// Newly authored fixture digest, not an original installation's hash.
fn install() -> ContentHash {
    ContentHash::from_bytes([0x22; 32])
}

fn digest(byte: u8) -> ContentHash {
    ContentHash::from_bytes([byte; 32])
}

fn namespace() -> MountNamespace {
    MountNamespace::new(NAMESPACE).expect("fixture namespace is valid")
}

fn key(path: &str) -> AssetKey {
    key_variant(path, "default")
}

fn key_variant(path: &str, variant: &str) -> AssetKey {
    AssetKey::from_spelling(NAMESPACE, path, variant).expect("fixture key is valid")
}

fn world(spelling: &str) -> WorldGroup {
    WorldGroup::new(spelling).expect("fixture world group is valid")
}

fn context() -> ResolveContext {
    ResolveContext::new(install())
}

fn in_world(spelling: &str) -> ResolveContext {
    context().with_world_group(world(spelling))
}

fn mod_stack(ids: &[&str]) -> ModStack {
    let mods = ids
        .iter()
        .map(|id| ModId::new(id).expect("fixture mod id is valid"))
        .collect();
    ModStack::new(mods).expect("fixture stacks do not repeat a mod")
}

fn mount(id: &str, container: &str, class: PrecedenceClass) -> MountBuilder {
    MountBuilder::new(
        MountId::new(id).expect("fixture mount id is valid"),
        namespace(),
        class,
        container,
    )
}

/// The fixture: three shared archives, two world archives, one patch
/// overlay, two mods, one mission source and one locale source — all in
/// one namespace, several of them holding the same logical paths on
/// purpose.
fn fixture() -> Vfs {
    let mut vfs = Vfs::new();

    let mut base = mount("shared.base", "fixture/base.zbd", PrecedenceClass::Shared);
    base.add_member("textures/hud/alert.dds", 64, 0, Some(digest(0xa1)))
        .expect("the shared alert texture indexes");
    base.add_member("textures/hud/shared_only.dds", 32, 64, Some(digest(0x51)))
        .expect("the shared-only texture indexes");
    base.add_member("textures/hud/contested.dds", 16, 96, Some(digest(0x52)))
        .expect("the contested texture indexes");
    base.add_member("textures/hud/ambiguous.dds", 8, 112, Some(digest(0x53)))
        .expect("the ambiguous texture indexes");
    vfs.mount(base.build().expect("the shared base mount builds"))
        .expect("the shared base mount registers");

    let mut extra = mount("shared.extra", "fixture/extra.zbd", PrecedenceClass::Shared);
    extra
        .add_member("textures/hud/ambiguous.dds", 8, 0, Some(digest(0x54)))
        .expect("the second ambiguous texture indexes");
    vfs.mount(extra.build().expect("the shared extra mount builds"))
        .expect(" the shared extra mount registers");

    let mut c1 = mount("world.c1", "fixture/c1.zbd", PrecedenceClass::MissionWorld)
        .with_world_group(world("zbd/c1"));
    c1.add_member("textures/hud/alert.dds", 64, 0, Some(digest(0xb1)))
        .expect("world c1's alert texture indexes");
    c1.add_member_variant(
        "textures/hud/alert.dds",
        AssetVariant::new("large").expect("fixture variant is valid"),
        256,
        64,
        Some(digest(0xc1)),
    )
    .expect("world c1's large variant indexes");
    vfs.mount(c1.build().expect("world c1 mount builds"))
        .expect("world c1 mount registers");

    let mut c2 = mount("world.c2", "fixture/c2.zbd", PrecedenceClass::MissionWorld)
        .with_world_group(world("zbd/c2"));
    c2.add_member("textures/hud/alert.dds", 64, 0, Some(digest(0xb2)))
        .expect("world c2's alert texture indexes");
    vfs.mount(c2.build().expect("world c2 mount builds"))
        .expect("world c2 mount registers");

    let mut patch = mount("patch.one", "fixture/patch.zbd", PrecedenceClass::Patch);
    patch
        .add_member("textures/hud/contested.dds", 16, 0, Some(digest(0x77)))
        .expect("the patched contested texture indexes");
    vfs.mount(patch.build().expect("the patch mount builds"))
        .expect("the patch mount registers");

    let mut hd = mount("mod.hd", "fixture/hd.zbd", PrecedenceClass::Mod)
        .with_mod(ModId::new("hd-pack").expect("fixture mod id is valid"));
    hd.add_member("textures/hud/contested.dds", 16, 0, Some(digest(0x99)))
        .expect("the hd pack's contested texture indexes");
    vfs.mount(hd.build().expect("the hd pack mount builds"))
        .expect("the hd pack mount registers");

    let mut hq = mount("mod.hq", "fixture/hq.zbd", PrecedenceClass::Mod)
        .with_mod(ModId::new("hq-pack").expect("fixture mod id is valid"));
    hq.add_member("textures/hud/contested.dds", 16, 0, Some(digest(0x88)))
        .expect("the hq pack's contested texture indexes");
    vfs.mount(hq.build().expect("the hq pack mount builds"))
        .expect("the hq pack mount registers");

    let mut mission = mount(
        "mission.m01",
        "fixture/m01.zbd",
        PrecedenceClass::MissionWorld,
    )
    .with_mission(MissionScope::new("m01").expect("fixture mission scope is valid"));
    mission
        .add_member("textures/hud/mission_only.dds", 12, 0, Some(digest(0x61)))
        .expect("the mission texture indexes");
    vfs.mount(mission.build().expect("the mission mount builds"))
        .expect("the mission mount registers");

    let mut french = mount("locale.fr", "fixture/fr.zbd", PrecedenceClass::Shared)
        .with_locale(LocaleLabel::new("french").expect("fixture locale is valid"));
    french
        .add_member("text/hud_labels.dds", 20, 0, Some(digest(0x62)))
        .expect("the french labels index");
    vfs.mount(french.build().expect("the locale mount builds"))
        .expect("the locale mount registers");

    vfs
}

/// **AC01.** Two worlds contain a same-named texture with different hashes;
/// each resolves its own version, and the losing world stays visible in
/// the trace instead of disappearing.
#[test]
fn accept_f04_a_two_worlds_each_resolve_own_texture() {
    let vfs = fixture();
    let alert = key("textures/hud/alert.dds");

    // Spelled differently from the mount's own `zbd/c1` on purpose: world
    // groups match logically, so `ZBD/C1` is the same world.
    let c1 = in_world("ZBD/C1");
    let c2 = in_world("zbd/c2");

    let from_c1 = vfs
        .resolve(&c1, &alert)
        .expect("world zbd/c1 has the texture");
    let from_c2 = vfs
        .resolve(&c2, &alert)
        .expect("world zbd/c2 has the texture");

    assert_eq!(
        from_c1.key, from_c2.key,
        "the two requests are one logical asset"
    );
    assert_eq!(from_c1.span.member_sha256(), Some(digest(0xb1)));
    assert_eq!(from_c2.span.member_sha256(), Some(digest(0xb2)));
    assert_ne!(
        from_c1.span.member_sha256(),
        from_c2.span.member_sha256(),
        "the same-named texture has different hashes per world"
    );
    assert_eq!(from_c1.span.container_path(), "fixture/c1.zbd");
    assert_eq!(from_c2.span.container_path(), "fixture/c2.zbd");
    assert_ne!(
        from_c1.span.container_path(),
        from_c2.span.container_path(),
        "equal basenames from different archives stay distinct"
    );
    assert_eq!(
        from_c1.span.member_key(),
        Some("textures/hud/alert.dds"),
        "the span records the member's original spelling"
    );
    assert_eq!(from_c1.span.install_sha256(), install());
    assert_eq!(from_c1.span.offset(), 0);
    assert_eq!(from_c1.span.length(), 64);
    assert_eq!(from_c1.mount.to_string(), "world.c1");
    assert_eq!(from_c2.mount.to_string(), "world.c2");
    assert_eq!(from_c1.precedence, PrecedenceClass::MissionWorld);

    let attempts = &from_c1.trace.attempts;
    assert_eq!(
        from_c1.trace.order,
        LookupOrderStatus {
            order: LookupOrder::Precedence,
            status: ClaimStatus::Designed,
        },
        "the ordering that decided this says it is designed, not measured"
    );
    let selected: Vec<&str> = attempts
        .iter()
        .filter(|attempt| attempt.outcome == AttemptOutcome::Selected)
        .map(|attempt| attempt.mount.as_str())
        .collect();
    assert_eq!(selected, ["world.c1"], "exactly one origin is selected");

    let ranks: Vec<u8> = attempts
        .iter()
        .map(|attempt| attempt.precedence.rank())
        .collect();
    assert!(
        ranks.windows(2).all(|pair| pair[0] >= pair[1]),
        "attempts are ordered by descending precedence, not by mount order: {ranks:?}"
    );
    assert!(
        attempts.iter().any(|attempt| {
            attempt.mount.as_str() == "world.c2"
                && attempt.outcome == AttemptOutcome::Skipped(SkipReason::ScopeMismatch)
        }),
        "the other world's mount is on record as skipped, not silently dropped"
    );
    assert!(
        attempts.iter().any(|attempt| {
            attempt.mount.as_str() == "shared.base" && attempt.outcome == AttemptOutcome::Candidate
        }),
        "the shared copy holds the key too but lost on precedence"
    );
}

/// A world-specific source only wins when a context selects that world:
/// otherwise the shared source serves the key, and every world mount is
/// still listed as an attempt.
#[test]
fn accept_f04_a_shared_source_serves_when_no_world_specializes() {
    let vfs = fixture();
    let alert = key("textures/hud/alert.dds");

    let plain = vfs
        .resolve(&context(), &alert)
        .expect("the shared copy serves a context with no world selected");
    assert_eq!(plain.span.container_path(), "fixture/base.zbd");
    assert_eq!(plain.span.member_sha256(), Some(digest(0xa1)));
    assert_eq!(plain.precedence, PrecedenceClass::Shared);
    for mount_id in ["world.c1", "world.c2"] {
        assert!(
            plain.trace.attempts.iter().any(|attempt| {
                attempt.mount.as_str() == mount_id
                    && attempt.outcome == AttemptOutcome::Skipped(SkipReason::ScopeMismatch)
            }),
            "{mount_id} must appear as a skipped attempt"
        );
    }

    let c1 = in_world("zbd/c1");
    let shared_only = vfs
        .resolve(&c1, &key("textures/hud/shared_only.dds"))
        .expect("a key only the shared mount holds still resolves in a world");
    assert_eq!(shared_only.span.container_path(), "fixture/base.zbd");
    assert_eq!(shared_only.span.member_sha256(), Some(digest(0x51)));
    assert!(
        shared_only.trace.attempts.iter().any(|attempt| {
            attempt.mount.as_str() == "world.c1" && attempt.outcome == AttemptOutcome::Miss
        }),
        "the world mount was consulted and did not hold the key"
    );
}

/// Spec F04 non-negotiable behavior 3: two equal-priority sources holding
/// one key is a diagnostic error carrying **both origins**, never a
/// first-wins pick.
#[test]
fn accept_f04_a_equal_priority_overlap_fails_with_both_origins() {
    let vfs = fixture();
    let ambiguous = key("textures/hud/ambiguous.dds");

    let err = vfs
        .resolve(&context(), &ambiguous)
        .expect_err("two shared mounts hold this key");

    let ResolveError::Ambiguous {
        key: asked,
        candidates,
        trace,
    } = &err
    else {
        panic!("an equal-priority overlap must be an ambiguity, not a guess: {err}");
    };

    assert_eq!(
        asked.logical_key(),
        "content/default/textures/hud/ambiguous.dds"
    );
    assert_eq!(candidates.len(), 2, "both origins are reported");
    let containers: Vec<&str> = candidates
        .iter()
        .map(|origin| origin.container.as_str())
        .collect();
    assert_eq!(containers, ["fixture/base.zbd", "fixture/extra.zbd"]);
    assert_eq!(candidates[0].mount.to_string(), "shared.base");
    assert_eq!(candidates[1].mount.to_string(), "shared.extra");
    assert_eq!(candidates[0].sha256, Some(digest(0x53)));
    assert_eq!(candidates[1].sha256, Some(digest(0x54)));
    assert!(
        candidates
            .iter()
            .all(|origin| origin.precedence == PrecedenceClass::Shared),
        "the tie is at equal precedence, which is why it is a tie"
    );
    assert!(!trace.attempts.is_empty(), "the failure keeps its trace");

    let rendered = err.to_string();
    assert!(
        rendered.contains("fixture/base.zbd") && rendered.contains("fixture/extra.zbd"),
        "the diagnostic names both origins: {rendered}"
    );
}

/// Spec F04 non-negotiable behavior 2: opt-in mods over patch overlays
/// over mission/world-specific sources over shared sources — and a mod the
/// context did not opt into is reported, not silently served.
#[test]
fn accept_f04_a_opted_in_mods_outrank_patch_and_shared() {
    let vfs = fixture();
    let contested = key("textures/hud/contested.dds");

    let patched = vfs
        .resolve(&context(), &contested)
        .expect("a patch overlay serves the key when no mod is opted into");
    assert_eq!(patched.span.container_path(), "fixture/patch.zbd");
    assert_eq!(patched.span.member_sha256(), Some(digest(0x77)));
    assert_eq!(patched.precedence, PrecedenceClass::Patch);
    assert!(
        patched.trace.attempts.iter().any(|attempt| {
            attempt.mount.as_str() == "mod.hd"
                && attempt.outcome == AttemptOutcome::Skipped(SkipReason::ModNotOptedIn)
        }),
        "the un-opted mod is visible in the trace, not silently ignored"
    );
    assert!(
        patched.trace.attempts.iter().any(|attempt| {
            attempt.mount.as_str() == "shared.base" && attempt.outcome == AttemptOutcome::Candidate
        }),
        "the shared copy also holds the key and lost on precedence"
    );

    let modded = vfs
        .resolve(&context().with_mods(mod_stack(&["hd-pack"])), &contested)
        .expect("the opted-in mod serves the key");
    assert_eq!(modded.span.container_path(), "fixture/hd.zbd");
    assert_eq!(modded.span.member_sha256(), Some(digest(0x99)));
    assert_eq!(modded.precedence, PrecedenceClass::Mod);
    assert!(
        modded.trace.attempts.iter().any(|attempt| {
            attempt.mount.as_str() == "patch.one" && attempt.outcome == AttemptOutcome::Candidate
        }),
        "the patch overlay still holds the key and lost to the mod"
    );
}

/// The mod stack is a load order: a later entry outranks an earlier one,
/// so two opted-in mods give one answer per stack order instead of a tie.
#[test]
fn accept_f04_a_later_mod_in_the_stack_outranks_earlier() {
    let vfs = fixture();
    let contested = key("textures/hud/contested.dds");

    let hd_first = vfs
        .resolve(
            &context().with_mods(mod_stack(&["hd-pack", "hq-pack"])),
            &contested,
        )
        .expect("two opted-in mods resolve");
    assert_eq!(
        hd_first.span.container_path(),
        "fixture/hq.zbd",
        "the later entry of the stack wins"
    );
    assert!(
        hd_first.trace.attempts.iter().any(|attempt| {
            attempt.mount.as_str() == "mod.hd" && attempt.outcome == AttemptOutcome::Candidate
        }),
        "the outranked mod is a candidate, not an ambiguity"
    );

    let hq_first = vfs
        .resolve(
            &context().with_mods(mod_stack(&["hq-pack", "hd-pack"])),
            &contested,
        )
        .expect("the reversed stack resolves");
    assert_eq!(
        hq_first.span.container_path(),
        "fixture/hd.zbd",
        "reversing the stack reverses the answer, so the order is honored"
    );
}

/// A mission-scoped source serves only its mission, and a context that
/// does not select the mission gets a visible failure with the attempt.
#[test]
fn accept_f04_a_mission_scoped_source_serves_only_its_mission() {
    let vfs = fixture();
    let mission_texture = key("textures/hud/mission_only.dds");

    let serving = context().with_mission(MissionScope::new("m01").expect("fixture scope is valid"));
    let resolved = vfs
        .resolve(&serving, &mission_texture)
        .expect("the mission source serves its own mission");
    assert_eq!(resolved.span.container_path(), "fixture/m01.zbd");
    assert_eq!(resolved.span.member_sha256(), Some(digest(0x61)));

    let err = vfs
        .resolve(&context(), &mission_texture)
        .expect_err("without the mission, nothing holds this key");
    let ResolveError::NotFound { trace, .. } = &err else {
        panic!("a missing key is a NotFound, not an ambiguity: {err}");
    };
    assert!(
        trace.attempts.iter().any(|attempt| {
            attempt.mount.as_str() == "mission.m01"
                && attempt.outcome == AttemptOutcome::Skipped(SkipReason::ScopeMismatch)
        }),
        "the mission mount explains why it did not serve: {trace}"
    );
}

/// A locale-bound source serves its locale only, and locale matching is
/// case-insensitive like every other legacy spelling here.
#[test]
fn accept_f04_a_locale_bound_mount_serves_only_its_locale() {
    let vfs = fixture();
    let labels = key("text/hud_labels.dds");

    let french =
        context().with_locale(LocaleLabel::new("FRENCH").expect("fixture locale is valid"));
    let resolved = vfs
        .resolve(&french, &labels)
        .expect("the french mount serves a french context");
    assert_eq!(resolved.span.container_path(), "fixture/fr.zbd");
    assert_eq!(resolved.span.member_sha256(), Some(digest(0x62)));

    let english =
        context().with_locale(LocaleLabel::new("english").expect("fixture locale is valid"));
    let err = vfs
        .resolve(&english, &labels)
        .expect_err("the french mount does not serve an english context");
    assert!(
        matches!(err, ResolveError::NotFound { .. }),
        "a locale miss is a visible failure: {err}"
    );
}

/// The variant is part of the key: two variants of one path are two
/// members with their own ranges, and a world that lacks a variant does
/// not silently serve the other one.
#[test]
fn accept_f04_a_variant_is_part_of_the_key() {
    let vfs = fixture();
    let default_key = key("textures/hud/alert.dds");
    let large_key = key_variant("textures/hud/alert.dds", "large");

    let c1 = in_world("zbd/c1");
    let default_member = vfs.resolve(&c1, &default_key).expect("c1 has the default");
    let large_member = vfs
        .resolve(&c1, &large_key)
        .expect("c1 has the large variant");
    assert_eq!(default_member.span.member_sha256(), Some(digest(0xb1)));
    assert_eq!(large_member.span.member_sha256(), Some(digest(0xc1)));
    assert_eq!(default_member.span.offset(), 0);
    assert_eq!(
        large_member.span.offset(),
        64,
        "variants are distinct members with their own byte ranges"
    );

    let c2 = in_world("zbd/c2");
    let err = vfs
        .resolve(&c2, &large_key)
        .expect_err("only world c1 carries the large variant");
    assert!(
        matches!(err, ResolveError::NotFound { .. }),
        "a missing variant is reported, not answered with the default: {err}"
    );
}

/// A case-only repeat inside one mount is refused with both spellings, so
/// a mount can never hold two entries that legacy lookup would treat as
/// one.
#[test]
fn accept_f04_a_case_only_duplicate_is_refused_with_both_spellings() {
    let mut builder = mount("shared.base", "fixture/base.zbd", PrecedenceClass::Shared);
    builder
        .add_member("Textures/HUD/Alert.dds", 64, 0, Some(digest(0xa1)))
        .expect("the first spelling indexes");

    let err = builder
        .add_member("textures/hud/alert.dds", 64, 0, Some(digest(0xa2)))
        .expect_err("a case-only repeat must not overwrite the first member");

    assert_eq!(
        err,
        MountError::DuplicateMember {
            logical_key: "default/textures/hud/alert.dds".to_owned(),
            first_spelling: "Textures/HUD/Alert.dds".to_owned(),
            second_spelling: "textures/hud/alert.dds".to_owned(),
        }
    );
    let rendered = err.to_string();
    assert!(
        rendered.contains("Textures/HUD/Alert.dds") && rendered.contains("textures/hud/alert.dds"),
        "the diagnostic names both spellings: {rendered}"
    );

    let mounted = builder.build().expect("the refused member left no debris");
    let kept = mounted
        .member(&key("textures/hud/alert.dds"))
        .expect("the first member is still indexed");
    assert_eq!(
        kept.sha256(),
        Some(digest(0xa1)),
        "the rejected second member did not replace the first"
    );
}

/// Spec F04 non-negotiable behavior 1, at the mount boundary: a member
/// spelling that could leave its container is refused, with the raw
/// spelling and the rule that rejected it, and the byte range is checked.
#[test]
fn accept_f04_a_escaping_member_spellings_are_refused_at_mount() {
    let mut builder = mount("shared.base", "fixture/base.zbd", PrecedenceClass::Shared);

    let rejected: [(&str, RelativePathError); 7] = [
        ("", RelativePathError::Empty),
        ("../../escape.dds", RelativePathError::ParentComponent),
        ("..\\escape.dds", RelativePathError::ParentComponent),
        ("/escape.dds", RelativePathError::Absolute),
        ("C:\\escape.dds", RelativePathError::Absolute),
        ("hud/./escape.dds", RelativePathError::CurrentComponent),
        ("hud\0escape.dds", RelativePathError::InteriorNul),
    ];
    for (spelling, reason) in rejected {
        let err = builder
            .add_member(spelling, 8, 0, None)
            .expect_err("an escaping spelling must not be mounted");
        assert_eq!(
            err,
            MountError::InvalidMemberPath {
                spelling: spelling.to_owned(),
                reason,
            },
            "spelling {spelling:?} is refused for the right reason"
        );
    }

    let overflow = builder
        .add_member("hud/range.dds", u64::MAX, 1, None)
        .expect_err("offset + size must stay inside the 64-bit range");
    assert_eq!(
        overflow,
        MountError::SpanOverflow {
            spelling: "hud/range.dds".to_owned(),
            offset: 1,
            length: u64::MAX,
        }
    );

    let mounted = builder
        .build()
        .expect("no refused member corrupted the mount");
    assert!(
        mounted.member(&key("hud/range.dds")).is_none(),
        "a refused member is not silently present"
    );
}

/// A lookup that finds nothing reports every attempt it made, and never
/// claims a winner.
#[test]
fn accept_f04_a_unknown_key_reports_every_attempt() {
    let vfs = fixture();
    let missing = key("textures/hud/missing.dds");

    let err = vfs
        .resolve(&in_world("zbd/c1"), &missing)
        .expect_err("nothing in the fixture holds this key");

    let ResolveError::NotFound { key: asked, trace } = &err else {
        panic!("a key nobody holds is a NotFound: {err}");
    };
    assert_eq!(
        asked.logical_key(),
        "content/default/textures/hud/missing.dds"
    );
    assert!(!trace.attempts.is_empty());
    assert!(
        trace.attempts.iter().all(|attempt| matches!(
            attempt.outcome,
            AttemptOutcome::Miss | AttemptOutcome::Skipped(_)
        )),
        "a failure never reports a selected or candidate origin: {trace}"
    );
    for mount_id in ["shared.base", "world.c1", "world.c2", "patch.one"] {
        assert!(
            trace
                .attempts
                .iter()
                .any(|attempt| attempt.mount.as_str() == mount_id),
            "{mount_id} must be on record in the failure trace"
        );
    }
    assert_eq!(
        trace.order,
        LookupOrderStatus {
            order: LookupOrder::Precedence,
            status: ClaimStatus::Designed,
        }
    );

    let rendered = err.to_string();
    assert!(
        rendered.contains("no mount holds"),
        "the message says what is missing: {rendered}"
    );
}

/// Mount construction invariants: a mod source must name its mod, a mod
/// binding may only sit on a mod source, containers must be usable and
/// one id names one mount.
#[test]
fn accept_f04_a_mount_construction_invariants_hold() {
    let unnamed_mod = mount("mod.x", "fixture/x.zbd", PrecedenceClass::Mod)
        .build()
        .expect_err("a mod source without its mod cannot be registered");
    assert_eq!(
        unnamed_mod,
        MountError::ModClassWithoutBinding {
            id: MountId::new("mod.x").expect("fixture mount id is valid")
        }
    );

    let misbound = mount("shared.x", "fixture/x.zbd", PrecedenceClass::Shared)
        .with_mod(ModId::new("hd-pack").expect("fixture mod id is valid"))
        .build()
        .expect_err("only a mod source may bind to a mod");
    assert_eq!(
        misbound,
        MountError::BindingWithoutModClass {
            id: MountId::new("shared.x").expect("fixture mount id is valid"),
            class: PrecedenceClass::Shared,
        }
    );

    let empty_container = mount("shared.y", "", PrecedenceClass::Shared)
        .build()
        .expect_err("an empty container locates nothing");
    assert_eq!(empty_container, MountError::EmptyContainer);

    let mut vfs = Vfs::new();
    let one = mount("shared.base", "fixture/base.zbd", PrecedenceClass::Shared)
        .build()
        .expect("the mount builds");
    vfs.mount(one.clone()).expect("the first mount registers");
    let duplicate = vfs
        .mount(one)
        .expect_err("a second mount with the same id is refused");
    assert_eq!(
        duplicate,
        MountError::DuplicateMountId {
            id: MountId::new("shared.base").expect("fixture mount id is valid")
        }
    );
}

/// The mount namespace is part of the key space: a mount in another
/// namespace holds the same logical path on purpose, is never consulted
/// for this lookup, never appears in the trace and cannot collide with an
/// in-namespace source.
#[test]
fn accept_f04_a_namespaces_partition_the_key_space() {
    let mut vfs = fixture();

    let mut sound = MountBuilder::new(
        MountId::new("sound.alerts").expect("fixture mount id is valid"),
        MountNamespace::new("sound").expect("fixture namespace is valid"),
        PrecedenceClass::Shared,
        "fixture/sound.zbd",
    );
    sound
        .add_member("textures/hud/alert.dds", 64, 0, Some(digest(0xee)))
        .expect("the sound-namespace copy indexes");
    let sound = sound.build().expect("the sound mount builds");
    assert!(
        sound.member(&key("textures/hud/alert.dds")).is_none(),
        "a mount answers only its own namespace, even when called directly"
    );
    vfs.mount(sound).expect("the sound mount registers");

    // The other namespace serves its own key: the same logical path under
    // a different namespace is a different asset.
    let sound_key = AssetKey::from_spelling("sound", "textures/hud/alert.dds", "default")
        .expect("fixture key is valid");
    let from_sound = vfs
        .resolve(&context(), &sound_key)
        .expect("the sound namespace holds its own copy");
    assert_eq!(from_sound.span.container_path(), "fixture/sound.zbd");
    assert_eq!(from_sound.span.member_sha256(), Some(digest(0xee)));
    assert_eq!(from_sound.mount.to_string(), "sound.alerts");

    // The content key never sees it: no cross-namespace tie, no
    // cross-namespace attempt.
    let content_key = key("textures/hud/alert.dds");
    let plain = vfs
        .resolve(&context(), &content_key)
        .expect("a foreign namespace may not turn this key ambiguous");
    assert_eq!(plain.span.container_path(), "fixture/base.zbd");
    assert!(
        plain.trace.attempts.iter().all(|attempt| {
            attempt.mount.as_str() != "sound.alerts" && attempt.container != "fixture/sound.zbd"
        }),
        "a mount in another namespace is not an attempt of this lookup: {}",
        plain.trace
    );

    // A key whose namespace nothing serves fails with that reason, not
    // with a guess from a namespace it did not ask for.
    let orphan =
        AssetKey::from_spelling("model", "hud/body.dff", "default").expect("fixture key is valid");
    let err = vfs
        .resolve(&context(), &orphan)
        .expect_err("no mount in this VFS serves the `model` namespace");
    let ResolveError::NotFound { trace, .. } = &err else {
        panic!("an unmounted namespace is a NotFound: {err}");
    };
    assert!(
        trace.attempts.is_empty(),
        "a namespace nobody mounts produces no attempts: {trace}"
    );
    assert!(
        err.to_string().contains("no mount serves that namespace"),
        "the failure names the unmounted namespace: {err}"
    );
}

/// Spec F04 AC02 (its collision half, in memory): two equal-priority
/// mounts hold one legacy path under different letter case, and the
/// lookup fails with **both** origins, each quoting its own spelling,
/// instead of flattening to a first-wins pick.
#[test]
fn accept_f04_a_case_only_collision_across_mounts_fails_with_both_origins() {
    let mut vfs = Vfs::new();

    let mut upper = mount("shared.upper", "fixture/upper.zbd", PrecedenceClass::Shared);
    upper
        .add_member("Textures/HUD/Alert.dds", 64, 0, Some(digest(0xd1)))
        .expect("the upper-case spelling indexes");
    vfs.mount(upper.build().expect("the upper mount builds"))
        .expect("the upper mount registers");

    let mut lower = mount("shared.lower", "fixture/lower.zbd", PrecedenceClass::Shared);
    lower
        .add_member("textures/hud/alert.dds", 64, 0, Some(digest(0xd2)))
        .expect("the lower-case spelling indexes");
    vfs.mount(lower.build().expect("the lower mount builds"))
        .expect("the lower mount registers");

    let asked = key("Textures\\HUD\\alert.DDS");
    let err = vfs
        .resolve(&context(), &asked)
        .expect_err("a case-only collision at equal priority is never a guess");

    let ResolveError::Ambiguous {
        key: reported,
        candidates,
        trace,
    } = &err
    else {
        panic!("an equal-priority case-only collision must be an ambiguity: {err}");
    };
    assert_eq!(
        reported.logical_key(),
        "content/default/textures/hud/alert.dds",
        "the two spellings are one logical key, which is why they collide"
    );
    assert_eq!(candidates.len(), 2, "both origins are reported");

    let spellings: Vec<&str> = candidates
        .iter()
        .map(|origin| origin.member_spelling.as_str())
        .collect();
    assert_eq!(
        spellings,
        ["Textures/HUD/Alert.dds", "textures/hud/alert.dds"],
        "each origin quotes the spelling its own container contained"
    );
    let containers: Vec<&str> = candidates
        .iter()
        .map(|origin| origin.container.as_str())
        .collect();
    assert_eq!(containers, ["fixture/upper.zbd", "fixture/lower.zbd"]);
    assert_eq!(candidates[0].sha256, Some(digest(0xd1)));
    assert_eq!(candidates[1].sha256, Some(digest(0xd2)));

    let rendered = err.to_string();
    assert!(
        rendered.contains("Textures/HUD/Alert.dds")
            && rendered.contains("textures/hud/alert.dds")
            && rendered.contains("fixture/upper.zbd")
            && rendered.contains("fixture/lower.zbd"),
        "the diagnostic names both spellings and both origins: {rendered}"
    );
    assert!(!trace.attempts.is_empty(), "the failure keeps its trace");
    assert_eq!(
        trace.order,
        LookupOrderStatus {
            order: LookupOrder::Precedence,
            status: ClaimStatus::Designed,
        },
        "the ordering that decided the tie says it is designed, not measured"
    );
}
