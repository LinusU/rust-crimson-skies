//! The original engine's **archive bindings**: the mission level, `gamez.zbd`,
//! `planes.zbd`, `cam_anim.zbd`, `mis_anim.zbd` and the one texture archive
//! per world (task #687, `docs/findings/2026-10-05-f04-d-original-lookup-order.md`
//! sections B, C, E and G).
//!
//! What the finding records, from the owner's static analysis of the pinned
//! executable:
//!
//! * the INTERP scripts name the archives in one order — the world's
//!   `cam_anim.zbd`, the mission's `mis_anim.zbd`, the texture directory, then
//!   `gamez.zbd` and `planes.zbd`;
//! * `gamez.zbd` is bound **per world group only**, `planes.zbd` comes from the
//!   ZBD **root**;
//! * both animation archives are loaded completely, in list order, and neither
//!   replaces the other; **none of these four falls back** to another directory
//!   level;
//! * exactly **one** texture archive is opened per world, from `ZBD/<world>`
//!   with the shared `ZBD` directory as the fallback.
//!
//! # What these tests hold the engine to
//!
//! Production [`cs_assets::vfs::binding`] records those bindings for one
//! [`ResolveContext`] ([`WorldLayout`]), including the **mission level** the
//! designed layout had no place for ([`mission_directories`],
//! [`SessionBuilder::mount_installation_missions`], [`MISSION_NAMESPACE`]).
//!
//! These bindings are read off the original executable's **code**, not a
//! runtime capture, so the module reports
//! [`BINDING_ORDER_STATUS`](cs_assets::vfs::BINDING_ORDER_STATUS) = `inferred`
//! and the designed [`PRECEDENCE_ORDER_STATUS`] stays `designed`. A binding is a
//! name the original opens at one path, not a competition between sources, so
//! the designed precedence order is not consulted here.
//!
//! **Which texture file** a world opens is *not* decided here: that is task
//! #352's measured budget-and-tier rule in `cs_content::textures`, which
//! depends on this crate and so cannot be called from it. [`TextureBinding`]
//! records where the search happens and holds the file that rule selected, and
//! refuses a second one — the original opens exactly one per world.
//!
//! The synthetic tests author a small installation through production
//! discovery ([`discover`]) and pin the bindings, the mission level's scope
//! refusals and the refusal paths. The `retail_` tests read `$CS_GAME_DIR`
//! read-only through the same production code and fail loudly without it; CI
//! skips them.

mod common;

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use common::TempTree;
use cs_assets::install::{self, Diagnosis, sha256};
use cs_assets::vfs::{
    ArchiveFamily, AttemptOutcome, BINDING_ORDER_STATUS, BindingError, BindingLevel, BindingRole,
    BoundArchive, MISSION_NAMESPACE, MissionDirectory, ResolveError, SessionBuilder, SkipReason,
    TextureBinding, WorldLayout, mission_directories,
};
use cs_types::asset_id::{
    AssetKey, AssetVariant, MissionScope, PRECEDENCE_ORDER_STATUS, ResolveContext, WorldGroup,
};
use cs_types::evidence::{ClaimStatus, ContentHash};
use cs_types::install::RelativePath;

/// The acceptance prefix of this task.
const PREFIX: &str = "accept_f04_d_order_archives_";

/// How many acceptance tests this suite holds: eleven synthetic and four
/// retail ones. The evidence harness checks the recorded run reports exactly
/// this many task tests, so a test renamed or dropped cannot pass unnoticed.
const TASK_TESTS: usize = 15;

// -------------------------------------------------------------- fixtures ---

/// The synthetic installation's fingerprint. Authored data, not a real
/// installation's hash: 64 hex digits, decoded through the production type so a
/// context can never carry an arbitrary string.
const FIXTURE_HASH: &str = "f0468d7c00000000000000000000000000000000000000000000000000a6c7ee";

fn fixture_hash() -> ContentHash {
    ContentHash::from_hex(FIXTURE_HASH).expect("the fixture fingerprint is a content hash")
}

/// A context selecting the fixture world `zbd/c1c` and, when given, a mission.
fn context(mission: Option<&str>) -> ResolveContext {
    let context = ResolveContext::new(fixture_hash())
        .with_world_group(WorldGroup::new("zbd/c1c").expect("a valid world group"));
    match mission {
        Some(mission) => {
            context.with_mission(MissionScope::new(mission).expect("a valid mission scope"))
        }
        None => context,
    }
}

/// The shared `ZBD` directory of the synthetic installation.
fn shared() -> RelativePath {
    RelativePath::new("zbd").expect("a valid shared directory")
}

/// Writes an installation shaped like the measured layout: two world groups,
/// each with its own `gamez.zbd` and `cam_anim.zbd`, `zbd/planes.zbd` at the
/// root, and one mission directory holding `mis_anim.zbd`.
///
/// The mission directory is spelled **lower case** (`zbd/c1c/m01`), while the
/// original installation spells it upper case (`zbd/C1C/M01`) and so does the
/// test above that checks discovery keeps the walk's spelling. That is
/// deliberate: the VFS compares member names by their case-folded logical key,
/// so a layout composed from a context's `MissionScope` label finds the file
/// either way — and writing the fixture this way means the assertions cannot
/// pass by accident through a case-insensitive host file system.
fn installation(label: &str) -> (TempTree, Diagnosis) {
    let tree = TempTree::new(label);
    for group in ["c1", "c1c"] {
        tree.write(&format!("zbd/{group}/gamez.zbd"), b"gamez");
        tree.write(&format!("zbd/{group}/cam_anim.zbd"), b"cam");
    }
    tree.write("zbd/planes.zbd", b"planes");
    tree.write("zbd/c1c/m01/mis_anim.zbd", b"mis");
    let found = install::discover(tree.root()).unwrap_or_else(|error| {
        panic!("the synthetic installation is discovered: {error}");
    });
    (tree, found.diagnosis)
}

// ------------------------------------------------------ the four bindings ---

/// The bindings of a world with a mission, exactly as the INTERP scripts name
/// them: the world's camera animation, the mission's animation, `gamez.zbd`,
/// then `planes.zbd` from the ZBD root.
#[test]
fn accept_f04_d_order_archives_the_bindings_are_named_in_the_scripts_order() {
    let layout = WorldLayout::for_context(&context(Some("m01")), &shared())
        .expect("a world with a mission binds its archives");

    let named: Vec<(&str, &str)> = layout
        .archives()
        .iter()
        .map(|archive| (archive.role().label(), archive.container().as_str()))
        .collect();
    assert_eq!(
        named,
        vec![
            ("camera_animation", "zbd/c1c/cam_anim.zbd"),
            ("mission_animation", "zbd/c1c/m01/mis_anim.zbd"),
            ("gamez", "zbd/c1c/gamez.zbd"),
            ("planes", "zbd/planes.zbd"),
        ],
        "the world's cam_anim.zbd, then the mission's mis_anim.zbd, then the world's \
         gamez.zbd, then planes.zbd from the ZBD root"
    );

    // The script order is a property of the roles themselves, so a caller can
    // compare two loads without trusting the vector order.
    let orders: Vec<u8> = layout
        .archives()
        .iter()
        .map(BoundArchive::script_order)
        .collect();
    assert_eq!(
        orders,
        vec![0, 1, 3, 4],
        "the texture role is not a named archive"
    );
    assert!(
        orders.windows(2).all(|pair| pair[0] < pair[1]),
        "the bindings come out in script order"
    );

    // The animation pair loads in that order, each archive in the family it is
    // loaded by, and neither is the other's file.
    let animation: Vec<&str> = layout
        .animation_archives()
        .map(|archive| archive.container().as_str())
        .collect();
    assert_eq!(
        animation,
        vec!["zbd/c1c/cam_anim.zbd", "zbd/c1c/m01/mis_anim.zbd"],
        "world cam_anim.zbd loads first, then the mission's mis_anim.zbd, each loaded completely"
    );
    let gamez: Vec<&str> = layout
        .gamez_archives()
        .map(|archive| archive.container().as_str())
        .collect();
    assert_eq!(
        gamez,
        vec!["zbd/c1c/gamez.zbd", "zbd/planes.zbd"],
        "the world's gamez.zbd is read before planes.zbd from the root"
    );
}

/// `gamez.zbd` is bound **per world group only**; it is never a mission-level
/// archive, and `planes.zbd` comes from the ZBD root whatever world is loaded.
#[test]
fn accept_f04_d_order_archives_gamez_is_per_world_group_and_planes_is_the_root() {
    let level_of = |layout: &WorldLayout, role: BindingRole| {
        layout
            .archive(role)
            .unwrap_or_else(|| panic!("{role} is bound"))
            .level()
    };

    for world in ["zbd/c1", "zbd/c1c", "zbd/c5"] {
        let context = ResolveContext::new(fixture_hash())
            .with_world_group(WorldGroup::new(world).expect("a valid world group"))
            .with_mission(MissionScope::new("m01").expect("a valid mission scope"));
        let layout = WorldLayout::for_context(&context, &shared())
            .unwrap_or_else(|error| panic!("{world} binds its archives: {error}"));

        assert_eq!(
            level_of(&layout, BindingRole::GameZ),
            BindingLevel::World,
            "gamez.zbd is bound at the world level for {world}, never at the mission level"
        );
        assert_eq!(
            level_of(&layout, BindingRole::Planes),
            BindingLevel::Root,
            "planes.zbd is read from the ZBD root for {world}"
        );
        assert_eq!(
            layout
                .archive(BindingRole::Planes)
                .expect("planes is bound")
                .container()
                .as_str(),
            "zbd/planes.zbd",
            "the same shared file serves every world"
        );
        assert_eq!(
            layout
                .archive(BindingRole::CameraAnimation)
                .expect("cam_anim is bound")
                .container()
                .as_str(),
            format!("{world}/cam_anim.zbd"),
            "each world's own cam_anim.zbd"
        );
    }

    // Two worlds never share one gamez.zbd, which is the per-world part.
    let c1 = WorldLayout::for_context(
        &ResolveContext::new(fixture_hash())
            .with_world_group(WorldGroup::new("zbd/c1").expect("a valid world group")),
        &shared(),
    )
    .expect("c1 binds its archives");
    let c1c = WorldLayout::for_context(
        &ResolveContext::new(fixture_hash())
            .with_world_group(WorldGroup::new("zbd/c1c").expect("a valid world group")),
        &shared(),
    )
    .expect("c1c binds its archives");
    assert_ne!(
        c1.archive(BindingRole::GameZ).expect("bound").container(),
        c1c.archive(BindingRole::GameZ).expect("bound").container(),
        "each world group resolves its own gamez.zbd"
    );
}

/// The mission level exists only for a context that names a mission: without
/// one there is no `mis_anim.zbd` binding, and a mission without a world cannot
/// bind anything at all.
#[test]
fn accept_f04_d_order_archives_the_mission_level_exists_only_for_a_mission() {
    let world_only = WorldLayout::for_context(&context(None), &shared())
        .expect("a world without a mission binds its world archives");
    assert!(
        world_only.archive(BindingRole::MissionAnimation).is_none(),
        "a load that names no mission binds no mission-level archive: the original's \
         MISSION_DIR is unset, so support\\init.gw never spells mis_anim.zbd"
    );
    assert_eq!(
        world_only.archives().len(),
        3,
        "camera animation, gamez and planes remain bound"
    );

    let with_mission = WorldLayout::for_context(&context(Some("m01")), &shared())
        .expect("a world with a mission binds its mission archive");
    assert_eq!(
        with_mission
            .archive(BindingRole::MissionAnimation)
            .expect("the mission archive is bound")
            .level(),
        BindingLevel::Mission,
        "mis_anim.zbd is the one binding at the mission level"
    );
    assert_eq!(
        with_mission
            .archive(BindingRole::MissionAnimation)
            .expect("bound")
            .container()
            .as_str(),
        "zbd/c1c/m01/mis_anim.zbd",
        "the mission archive lives two levels below the shared directory"
    );

    // The levels are the directory depth, so nothing claims the mission archive
    // is "higher priority" than the world's.
    assert_eq!(BindingLevel::Root.depth(), 0);
    assert_eq!(BindingLevel::World.depth(), 1);
    assert_eq!(BindingLevel::Mission.depth(), 2);

    // Two missions of one world bind two different archives.
    let m01 = WorldLayout::for_context(&context(Some("m01")), &shared()).expect("m01");
    let m02 = WorldLayout::for_context(&context(Some("m02")), &shared()).expect("m02");
    assert_ne!(
        m01.archive(BindingRole::MissionAnimation)
            .expect("bound")
            .container(),
        m02.archive(BindingRole::MissionAnimation)
            .expect("bound")
            .container(),
        "each mission binds its own mis_anim.zbd"
    );
    assert_eq!(
        m01.archive(BindingRole::CameraAnimation)
            .expect("bound")
            .container(),
        m02.archive(BindingRole::CameraAnimation)
            .expect("bound")
            .container(),
        "the world's cam_anim.zbd is bound by both missions of that world"
    );

    // A mission with no world cannot be spelled: every binding lives below a
    // world group directory.
    let orphan = ResolveContext::new(fixture_hash())
        .with_mission(MissionScope::new("m01").expect("a valid mission scope"));
    assert_eq!(
        WorldLayout::for_context(&orphan, &shared()),
        Err(BindingError::NoWorldGroup {
            mission: Some("m01".to_owned())
        }),
        "a mission without a world names no container, and is refused naming the mission"
    );
    assert_eq!(
        WorldLayout::for_context(&ResolveContext::new(fixture_hash()), &shared()),
        Err(BindingError::NoWorldGroup { mission: None }),
        "a context with no world at all is refused too"
    );
}

/// The four named archives are looked up at their explicit path with **no**
/// directory-level fallback; the texture archive is the only one this module
/// records that falls back, and it falls back to the shared directory only.
#[test]
fn accept_f04_d_order_archives_only_the_texture_archive_falls_back_to_the_shared_directory() {
    for role in BindingRole::ALL {
        let expected = role == BindingRole::TextureArchive;
        assert_eq!(
            role.has_directory_fallback(),
            expected,
            "{} {}",
            role,
            if expected {
                "searches the world directory then the shared one"
            } else {
                "is named by an explicit path, with no fallback"
            }
        );
    }

    let layout = WorldLayout::for_context(&context(Some("m01")), &shared()).expect("bound");
    let texture = layout.texture();
    assert_eq!(
        texture
            .directories()
            .iter()
            .map(|directory| directory.as_str())
            .collect::<Vec<_>>(),
        vec!["zbd/c1c", "zbd"],
        "the world's own directory first, then the shared ZBD directory"
    );
    assert!(
        texture.falls_back(),
        "the texture search is the one level fallback the original makes among these archives"
    );
    assert!(
        !texture
            .directories()
            .iter()
            .any(|directory| directory.as_str() == "zbd/c1"),
        "an archive can only come from the selected world or from the shared directory, never \
         from another world"
    );

    // The shared directory is a fallback for the *texture search* only; the
    // named archives are still spelled under the world group.
    assert!(
        layout.archives().iter().all(
            |archive| archive.container().as_str().starts_with("zbd/c1c")
                || archive.role() == BindingRole::Planes
        ),
        "no named archive comes from another world's directory or from a fallback level"
    );
}

/// Exactly one texture archive per world: the binding starts empty, takes the
/// file the measured selection rule chose, and refuses a second one rather
/// than storing a tier-to-tier fallback the original does not have.
#[test]
fn accept_f04_d_order_archives_one_texture_archive_is_bound_per_world() {
    let texture = TextureBinding::new(
        RelativePath::new("zbd/c1c").expect("a valid world directory"),
        RelativePath::new("zbd").expect("a valid shared directory"),
    );
    assert_eq!(
        texture.archive(),
        None,
        "no file is bound before the selection rule runs"
    );

    let mut bound = texture;
    bound
        .bind(RelativePath::new("zbd/c1c/rtexture15.zbd").expect("a valid archive"))
        .expect("the first archive binds");
    assert_eq!(
        bound.archive().map(|archive| archive.as_str()),
        Some("zbd/c1c/rtexture15.zbd"),
        "the rule's choice is recorded"
    );
    assert_eq!(
        bound.bind(RelativePath::new("zbd/c1c/texture.zbd").expect("a valid archive")),
        Err(BindingError::TextureArchiveAlreadyBound {
            bound: "zbd/c1c/rtexture15.zbd".to_owned(),
            second: "zbd/c1c/texture.zbd".to_owned(),
        }),
        "a second tier is refused: the original opens exactly one archive per world and a \
         name it does not hold falls through to rimage.zbd, never to another tier"
    );
    assert_eq!(
        bound.archive().map(|archive| archive.as_str()),
        Some("zbd/c1c/rtexture15.zbd"),
        "the refused tier does not replace the bound one"
    );

    // The layout hands the binding out mutably so the rule can fill it in, and
    // the texture role is not one of the named archives.
    let mut layout = WorldLayout::for_context(&context(None), &shared()).expect("bound");
    assert!(layout.archive(BindingRole::TextureArchive).is_none());
    layout
        .texture_mut()
        .bind(RelativePath::new("zbd/c1c/texture.zbd").expect("a valid archive"))
        .expect("the software renderer's archive binds");
    assert_eq!(
        layout.texture().archive().map(|archive| archive.as_str()),
        Some("zbd/c1c/texture.zbd"),
        "which tier binds is the selection rule's call (#352), recorded here"
    );
}

/// The roles are the whole set, and each one is reachable by its stable label —
/// so a report or a trace can name a binding without this crate's internals.
#[test]
fn accept_f04_d_order_archives_every_role_is_reachable_by_its_label() {
    assert_eq!(BindingRole::ALL.len(), 5);
    for role in BindingRole::ALL {
        assert_eq!(
            BindingRole::from_label(role.label()),
            Some(role),
            "the label round-trips"
        );
        assert_eq!(role.to_string(), role.label());
        assert!(
            role.file().is_some() || role == BindingRole::TextureArchive,
            "every role but the texture archive has a fixed file name"
        );
    }
    assert_eq!(BindingRole::from_label("nothing"), None);
    for family in [
        ArchiveFamily::GameZ,
        ArchiveFamily::Animation,
        ArchiveFamily::Texture,
    ] {
        assert!(!family.label().is_empty());
    }
    for level in [
        BindingLevel::Root,
        BindingLevel::World,
        BindingLevel::Mission,
    ] {
        assert!(!level.label().is_empty());
        assert_eq!(level.to_string(), level.label());
    }
    assert_eq!(
        BindingRole::CameraAnimation.family(),
        ArchiveFamily::Animation,
        "cam_anim.zbd is loaded by the animation family"
    );
    assert_eq!(
        BindingRole::Planes.family(),
        ArchiveFamily::GameZ,
        "planes.zbd is read by GameZReadZBDFile"
    );
}

/// The bindings are code-derived evidence, not a runtime capture: the module
/// says so, and the designed precedence order is left alone.
#[test]
fn accept_f04_d_order_archives_the_bindings_are_code_derived_and_the_designed_order_untouched() {
    assert_eq!(
        BINDING_ORDER_STATUS,
        ClaimStatus::Inferred,
        "the bindings are read off the original executable's code, which is evidence but not a \
         runtime capture, so they are never presented as verified_original"
    );
    assert_ne!(
        BINDING_ORDER_STATUS,
        ClaimStatus::VerifiedOriginal,
        "no binding here may claim a measured original run"
    );
    assert_eq!(
        PRECEDENCE_ORDER_STATUS,
        ClaimStatus::Designed,
        "the designed precedence order is untouched: a binding names one explicit path and \
         consults no precedence class"
    );
    assert_eq!(
        WorldLayout::for_context(&context(Some("m01")), &shared())
            .expect("bound")
            .order_status(),
        BINDING_ORDER_STATUS,
        "a layout reports the status of the bindings it records"
    );
}

// ------------------------------------------------------------ the layout ---

/// `missing` lists what this installation does not hold, so a layout naming an
/// archive retail does not ship is visible rather than silent.
#[test]
fn accept_f04_d_order_archives_a_binding_this_installation_lacks_is_listed() {
    let (tree, diagnosis) = installation("archives-missing");
    let layout = WorldLayout::for_context(&context(Some("m01")), &shared()).expect("bound");

    assert_eq!(
        layout.missing(tree.root()),
        Vec::<String>::new(),
        "the synthetic installation holds every archive the layout binds"
    );

    // A mission the installation does not have: the mission archive is named
    // as missing, the world's archives are not.
    let absent = WorldLayout::for_context(&context(Some("m99")), &shared()).expect("bound");
    assert_eq!(
        absent.missing(tree.root()),
        vec!["zbd/c1c/m99/mis_anim.zbd".to_owned()],
        "only the archive this installation does not ship is named"
    );

    // A world group with no archives at all names all of its own bindings.
    let empty = WorldLayout::for_context(
        &ResolveContext::new(fixture_hash())
            .with_world_group(WorldGroup::new("zbd/c4").expect("a valid world group")),
        &shared(),
    )
    .expect("bound");
    let missing = empty.missing(tree.root());
    assert!(
        missing.contains(&"zbd/c4/gamez.zbd".to_owned()),
        "{missing:?}"
    );
    assert!(
        missing.contains(&"zbd/c4/cam_anim.zbd".to_owned()),
        "{missing:?}"
    );
    assert!(
        !missing.contains(&"zbd/c4/zbd".to_owned()),
        "the texture search directories are checked as directories, not as archives"
    );

    // The check is a host listing: it never writes into the installation, and
    // the tree still holds exactly what it was given.
    assert!(tree.root().join("zbd/c1c/gamez.zbd").is_file());
    assert_eq!(
        diagnosis.world_groups.len(),
        2,
        "the fixture installation has two world groups, so `missing` ran against a real one"
    );
}

// ---------------------------------------------------------- mission level ---

/// The mission level is derived from **discovery**, from the directories the
/// walk observed — not from a mission-name table — so a mission the campaign
/// tables do not claim is still mounted, and a directory that carries no file
/// is still a mission directory.
#[test]
fn accept_f04_d_order_archives_mission_directories_come_from_discovery_not_a_name_table() {
    let tree = TempTree::new("archives-mission-dirs");
    tree.write("zbd/c1c/gamez.zbd", b"gamez");
    // Spelled as the installation spells a mission directory, so the walk's
    // own spelling is what the rows carry.
    tree.write("zbd/c1c/M01/mis_anim.zbd", b"mis");
    // A mission directory the campaign mission tables (m01..m05, mp1..mp5,
    // ia1) do not name, plus an empty one and a directory one level too deep.
    tree.write("zbd/c1c/ZZ9/mis_anim.zbd", b"mis");
    tree.write("zbd/c1c/EMPTY/.keep", b"");
    tree.write("zbd/c1c/M01/nets/deep.zbd", b"deep");
    let found = install::discover(tree.root()).expect("the fixture installation is discovered");

    let missions = mission_directories(&found.diagnosis);
    let spelled: Vec<&str> = missions
        .iter()
        .map(|mission| mission.directory.as_str())
        .collect();
    assert_eq!(
        spelled,
        vec!["zbd/c1c/EMPTY", "zbd/c1c/M01", "zbd/c1c/ZZ9"],
        "every directory directly below a world group is a mission directory, in logical \
         order; a directory two levels deeper is not, and a table that omits zz9 does not \
         remove it"
    );

    let m01 = &missions[1];
    assert_eq!(
        m01.mission, "M01",
        "the installation's own spelling is kept"
    );
    assert_eq!(m01.world_group.as_str(), "zbd/c1c");
    assert_eq!(
        m01.scope().expect("M01 is a valid mission label").as_str(),
        "m01",
        "the mission scope is the directory name folded to a label"
    );
    assert_eq!(
        m01.world().as_relative().as_str(),
        "zbd/c1c",
        "the mission's world group is the group it was found under"
    );
    assert_eq!(m01.to_string(), "zbd/c1c/M01");
    assert_eq!(missions[0].mission, "EMPTY");
    assert_eq!(missions[2].mission, "ZZ9");
}

/// Mounting the mission level binds each mission directory to its own world and
/// mission: the selected mission resolves its `mis_anim.zbd`, a sibling mission
/// and another world are refused it, and the world's own archives stay in the
/// world namespace.
#[test]
fn accept_f04_d_order_archives_the_mission_level_is_mounted_for_its_own_mission() {
    let (tree, diagnosis) = installation("archives-mission-mount");
    let mut builder = SessionBuilder::new(context(Some("m01")));
    builder
        .mount_installation(tree.root(), &diagnosis)
        .expect("the installation mounts");
    builder
        .mount_installation_missions(tree.root(), &diagnosis)
        .expect("the mission level mounts");
    let session = builder.open();

    // One mission directory, so one mission mount, in its own namespace.
    let missions: Vec<String> = session
        .mounts()
        .filter(|mount| mount.namespace().as_str() == MISSION_NAMESPACE)
        .map(|mount| mount.container().to_owned())
        .collect();
    assert_eq!(
        missions,
        vec!["zbd/c1c/m01".to_owned()],
        "the mission directory is mounted under the mission namespace"
    );

    let key = |name: &str| {
        AssetKey::from_spelling(MISSION_NAMESPACE, name, AssetVariant::default().as_str())
            .expect("a valid mission key")
    };

    // The selected mission resolves its own archive from its own mount.
    let asset = session
        .resolve(&key("mis_anim.zbd"))
        .expect("the selected mission resolves mis_anim.zbd");
    assert_eq!(
        asset.resolved().span.container_path(),
        "zbd/c1c/m01",
        "the bytes come from the mission's own directory"
    );
    assert_eq!(
        session.read_all(&asset).expect("the archive is read"),
        b"mis".to_vec(),
        "the mission's own bytes, not a sibling's"
    );

    // Another mission of the same world is refused it: the mount is bound to
    // one mission.
    let mut sibling_builder = SessionBuilder::new(context(Some("m02")));
    sibling_builder
        .mount_installation(tree.root(), &diagnosis)
        .expect("the installation mounts");
    sibling_builder
        .mount_installation_missions(tree.root(), &diagnosis)
        .expect("the mission level mounts");
    let sibling = sibling_builder.open();
    let refused = sibling
        .resolve(&key("mis_anim.zbd"))
        .expect_err("a mission the installation does not hold resolves nothing");
    let trace = match &refused {
        ResolveError::NotFound { trace, .. } => trace,
        other => panic!("a scope mismatch is a miss, not {other:?}"),
    };
    assert!(
        !trace.attempts.is_empty()
            && trace.attempts.iter().all(
                |attempt| attempt.outcome == AttemptOutcome::Skipped(SkipReason::ScopeMismatch)
            ),
        "every attempt is a scope mismatch: the mission mount belongs to m01, not to m02: {trace}"
    );

    // Another world is refused it too, which is the world half of the binding.
    let mut other_builder = SessionBuilder::new(
        ResolveContext::new(fixture_hash())
            .with_world_group(WorldGroup::new("zbd/c1").expect("a valid world group"))
            .with_mission(MissionScope::new("m01").expect("a valid mission scope")),
    );
    other_builder
        .mount_installation(tree.root(), &diagnosis)
        .expect("the installation mounts");
    other_builder
        .mount_installation_missions(tree.root(), &diagnosis)
        .expect("the mission level mounts");
    let other_world = other_builder.open();
    assert!(
        other_world.resolve(&key("mis_anim.zbd")).is_err(),
        "world c1's context is refused c1c's mission archive"
    );

    // The world's own archives stay in the world namespace; the mission mount
    // never answers for them.
    let world_key = AssetKey::from_spelling("world", "gamez.zbd", AssetVariant::default().as_str())
        .expect("a valid world key");
    let world_asset = session
        .resolve(&world_key)
        .expect("the world namespace still serves gamez.zbd");
    assert_eq!(world_asset.resolved().span.container_path(), "zbd/c1c");
    assert_eq!(
        session.read_all(&world_asset).expect("read"),
        b"gamez".to_vec()
    );
}

/// The designed installation layout mounts the **world** level only, and says
/// so: the mission level is a separate, explicit step, so nothing binds a
/// mission archive a caller did not ask for.
#[test]
fn accept_f04_d_order_archives_the_world_layout_alone_binds_no_mission() {
    let (tree, diagnosis) = installation("archives-world-only");
    let mut builder = SessionBuilder::new(context(Some("m01")));
    builder
        .mount_installation(tree.root(), &diagnosis)
        .expect("the installation mounts");
    let session = builder.open();

    assert!(
        session
            .mounts()
            .all(|mount| mount.namespace().as_str() != MISSION_NAMESPACE),
        "mount_installation binds the world level only"
    );
    assert!(
        session
            .resolve(
                &AssetKey::from_spelling(
                    MISSION_NAMESPACE,
                    "mis_anim.zbd",
                    AssetVariant::default().as_str(),
                )
                .expect("a valid mission key"),
            )
            .is_err(),
        "without the mission step a mission archive has no mount at all"
    );
    // …while the world's own binding still resolves.
    let world_asset = session
        .resolve(
            &AssetKey::from_spelling("world", "cam_anim.zbd", AssetVariant::default().as_str())
                .expect("a valid world key"),
        )
        .expect("the world's cam_anim.zbd resolves");
    assert_eq!(
        world_asset.resolved().span.container_path(),
        "zbd/c1c",
        "each world resolves its own cam_anim.zbd from its own directory"
    );
}

// ------------------------------------------------------------------ retail ---

/// `$CS_GAME_DIR`, or a loud failure naming what is missing.
fn retail_root() -> PathBuf {
    let raw = std::env::var("CS_GAME_DIR")
        .expect("CS_GAME_DIR must point at the original installation for this test");
    let root = PathBuf::from(raw);
    assert!(
        root.join("ZBD").is_dir(),
        "CS_GAME_DIR must point at the original installation for this test: {} is not one",
        root.display()
    );
    root
}

/// The installation, discovered through production code once per process.
fn retail_diagnosis() -> &'static Diagnosis {
    static DIAGNOSIS: std::sync::OnceLock<Diagnosis> = std::sync::OnceLock::new();
    DIAGNOSIS.get_or_init(|| {
        let root = retail_root();
        install::discover(&root)
            .unwrap_or_else(|error| panic!("the installation is discovered: {error}"))
            .diagnosis
    })
}

/// The installation's shared directory as discovery spells it.
fn retail_shared() -> RelativePath {
    retail_diagnosis()
        .zbd_dir
        .clone()
        .expect("the installation holds its ZBD directory")
}

/// Every world group the installation ships binds exactly the measured
/// archives: `gamez.zbd` and `cam_anim.zbd` in its own directory, `planes.zbd`
/// once from the ZBD root, `mis_anim.zbd` in every mission directory it has,
/// and one texture search over its own directory then the shared one.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f04_d_order_archives_retail_world_groups_bind_the_measured_archives() {
    let root = retail_root();
    let diagnosis = retail_diagnosis();
    let shared = retail_shared();
    assert_eq!(
        diagnosis.world_groups.len(),
        8,
        "the eight world groups C1, C1B, C1C, C2, C2B, C3, C4 and C5"
    );
    assert!(
        diagnosis.planes_zbd.is_some(),
        "ZBD/planes.zbd is inventoried"
    );

    for group in &diagnosis.world_groups {
        let world = WorldGroup::from_relative(group.clone());
        let layout = WorldLayout::for_context(
            &ResolveContext::new(sha256(&[])).with_world_group(world.clone()),
            &shared,
        )
        .unwrap_or_else(|error| panic!("{} binds its archives: {error}", group.as_str()));

        assert_eq!(
            layout
                .archive(BindingRole::GameZ)
                .expect("gamez is bound")
                .container()
                .as_str(),
            format!("{}/gamez.zbd", group.as_str()),
            "gamez.zbd is bound per world group"
        );
        assert_eq!(
            layout
                .archive(BindingRole::CameraAnimation)
                .expect("cam_anim is bound")
                .container()
                .as_str(),
            format!("{}/cam_anim.zbd", group.as_str()),
            "the world's own cam_anim.zbd"
        );
        assert_eq!(
            layout
                .archive(BindingRole::Planes)
                .expect("planes is bound")
                .container()
                .as_str(),
            format!("{}/planes.zbd", shared.as_str()),
            "planes.zbd is read from the ZBD root"
        );
        assert_eq!(
            layout.missing(&root),
            Vec::<String>::new(),
            "{} binds only archives the installation actually ships",
            group.as_str()
        );

        // A mission of that world adds exactly one binding, and it exists.
        let missions = mission_directories(diagnosis)
            .into_iter()
            .filter(|mission| mission.world_group == *group)
            .collect::<Vec<MissionDirectory>>();
        for mission in &missions {
            let mission_layout = WorldLayout::for_context(
                &ResolveContext::new(sha256(&[]))
                    .with_world_group(world.clone())
                    .with_mission(
                        mission
                            .scope()
                            .unwrap_or_else(|error| panic!("{} is a label: {error}", mission)),
                    ),
                &shared,
            )
            .unwrap_or_else(|error| {
                panic!(
                    "{} binds its mission archives: {error}",
                    mission.directory.as_str()
                )
            });
            // The container is composed from the context's `MissionScope`
            // label, which is lower case (`ia1`) while the installation spells
            // the directory `IA1`; the comparison is therefore the logical key,
            // exactly as a mount compares members. `mission_directories` keeps
            // the walk's own spelling, checked above.
            assert_eq!(
                mission_layout
                    .archive(BindingRole::MissionAnimation)
                    .expect("mis_anim is bound")
                    .container()
                    .logical_key(),
                format!(
                    "{}/{}/mis_anim.zbd",
                    group.logical_key(),
                    mission
                        .directory
                        .logical_key()
                        .rsplit('/')
                        .next()
                        .expect("a mission name")
                ),
                "the mission's own mis_anim.zbd, below its own directory"
            );
            assert_eq!(
                mission_layout.missing(&root),
                Vec::<String>::new(),
                "{} binds only archives the installation ships",
                mission.directory.as_str()
            );
        }
    }
}

/// What the measurement records, measured on this installation: `gamez.zbd` is
/// per world group only (no mission directory has one), and every mission
/// directory has a `mis_anim.zbd`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f04_d_order_archives_retail_gamez_is_never_per_mission_and_mis_anim_always_is() {
    let root = retail_root();
    let diagnosis = retail_diagnosis();
    let missions = mission_directories(diagnosis);
    assert!(
        !missions.is_empty(),
        "the installation holds mission directories below its world groups"
    );

    let mut with_gamez = Vec::new();
    let mut without_mis_anim = Vec::new();
    for mission in &missions {
        let directory = root.join(mission.directory.as_str());
        if directory.join("gamez.zbd").is_file() {
            with_gamez.push(mission.directory.as_str().to_owned());
        }
        if !directory.join("mis_anim.zbd").is_file() {
            without_mis_anim.push(mission.directory.as_str().to_owned());
        }
        // The camera animation archive is the world's, never the mission's.
        assert!(
            !directory.join("cam_anim.zbd").is_file(),
            "{} also holds a cam_anim.zbd, which the measured layout does not bind at the \
             mission level",
            mission.directory.as_str()
        );
    }
    assert!(
        with_gamez.is_empty(),
        "no mission directory ships its own gamez.zbd: it is bound per world group only, \
         found one in {with_gamez:?}"
    );
    assert!(
        without_mis_anim.is_empty(),
        "every mission directory ships a mis_anim.zbd, missing one in {without_mis_anim:?}"
    );

    // And every world group has both of its world-level archives.
    for group in &diagnosis.world_groups {
        for file in ["gamez.zbd", "cam_anim.zbd"] {
            assert!(
                root.join(group.as_str()).join(file).is_file(),
                "{}/{file} is missing",
                group.as_str()
            );
        }
    }
}

/// The texture search of every world group is its own directory plus the
/// shared one, and the shared directory is where `rimage.zbd` comes from —
/// never another world's directory.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f04_d_order_archives_retail_each_world_searches_its_own_directory_then_the_shared_one() {
    let root = retail_root();
    let diagnosis = retail_diagnosis();
    let shared = retail_shared();
    assert!(
        root.join(shared.as_str()).join("rimage.zbd").is_file(),
        "the shared directory holds rimage.zbd, which the same search finds"
    );

    for group in &diagnosis.world_groups {
        let layout = WorldLayout::for_context(
            &ResolveContext::new(sha256(&[]))
                .with_world_group(WorldGroup::from_relative(group.clone())),
            &shared,
        )
        .expect("bound");
        let directories: Vec<&str> = layout
            .texture()
            .directories()
            .iter()
            .map(|directory| directory.as_str())
            .collect();
        assert_eq!(
            directories,
            vec![group.as_str(), shared.as_str()],
            "{} searches its own directory first, then the shared one",
            group.as_str()
        );
        // Every tier the rule may choose is in the world's own directory, so
        // the search can only reach this world or the shared directory.
        let tiers: Vec<String> = fs::read_dir(root.join(group.as_str()))
            .unwrap_or_else(|error| panic!("{} is listed: {error}", group.as_str()))
            .filter_map(|entry| {
                let name = entry.ok()?.file_name().to_string_lossy().into_owned();
                (name.ends_with(".zbd")).then_some(name)
            })
            .collect();
        assert!(
            tiers.iter().any(|name| name == "texture.zbd"),
            "{} ships texture.zbd, the unnumbered name the walk ends at: {tiers:?}",
            group.as_str()
        );
    }
}

/// The mission level as **mounts**: every mission directory of the installation
/// is mounted under [`MISSION_NAMESPACE`], bound to its own world and mission,
/// and each mission's `mis_anim.zbd` reads back from its own directory through
/// production code.
///
/// The **whole** mount list is built once, with one mission's context, and
/// every mission mount is checked there. The read-back then walks the 53
/// mission directories one at a time with a [`Diagnosis`] narrowed to that one
/// directory, because [`SessionBuilder::mount_installation`] would otherwise
/// re-hash the entire 600+ MB installation 53 times; the mission mount itself
/// is the production code under test either way, and the world's own archives
/// are covered by the test above.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f04_d_order_archives_retail_the_mission_level_mounts_every_mission_directory() {
    let root = retail_root();
    let diagnosis = retail_diagnosis();
    let missions = mission_directories(diagnosis);
    let expected = missions
        .iter()
        .map(|mission| mission.directory.logical_key())
        .collect::<Vec<_>>();
    assert_eq!(
        expected.len(),
        diagnosis
            .directories
            .iter()
            .filter(|directory| {
                let key = directory.logical_key();
                key.matches('/').count() == 2 && key.starts_with("zbd/")
            })
            .count(),
        "every mission directory discovery observed is a mission directory of a world group"
    );

    // The whole mount list, built once.
    let first = missions
        .first()
        .expect("the installation holds at least one mission directory");
    let mut builder = SessionBuilder::new(context_for(first));
    builder
        .mount_installation(&root, diagnosis)
        .expect("the installation mounts");
    builder
        .mount_installation_missions(&root, diagnosis)
        .expect("the mission level mounts");
    let session = builder.open();
    let mounted: Vec<String> = session
        .mounts()
        .filter(|mount| mount.namespace().as_str() == MISSION_NAMESPACE)
        .map(|mount| mount.container().to_lowercase())
        .collect();
    assert_eq!(
        mounted, expected,
        "every mission directory the walk found is mounted under the mission namespace"
    );
    // Each mission mount is bound to **its own** world group and mission, so no
    // context is admitted to another mission's archives. The bindings are
    // compared by logical key, because a mission mount carries the walk's own
    // spelling (`IA1`) while a `MissionScope` label is lower case (`ia1`).
    for mount in session
        .mounts()
        .filter(|mount| mount.namespace().as_str() == MISSION_NAMESPACE)
    {
        let container = RelativePath::new(mount.container()).unwrap_or_else(|error| {
            panic!("{} is a relative spelling: {error}", mount.container())
        });
        let key = container.logical_key();
        let own = missions
            .iter()
            .find(|mission| mission.directory.logical_key() == key)
            .unwrap_or_else(|| panic!("{key} is a mission directory the walk found"));
        assert_eq!(
            mount
                .scope()
                .world_group
                .as_ref()
                .map(WorldGroup::logical_key),
            Some(own.world_group.logical_key()),
            "{key} is bound to its own world group"
        );
        assert_eq!(
            mount
                .scope()
                .mission
                .as_ref()
                .map(MissionScope::as_str)
                .map(str::to_owned),
            Some(
                own.scope()
                    .expect("the mission name is a label")
                    .as_str()
                    .to_owned()
            ),
            "{key} is bound to its own mission"
        );
    }

    // One read-back per mission directory, through the production mount.
    let mut checked = 0usize;
    for mission in &missions {
        let mut narrowed = diagnosis.clone();
        narrowed
            .directories
            .retain(|directory| directory.logical_key() == mission.directory.logical_key());
        let mut builder = SessionBuilder::new(context_for(mission));
        builder
            .mount_installation_missions(&root, &narrowed)
            .expect("the mission level mounts");
        let session = builder.open();
        let key = AssetKey::from_spelling(
            MISSION_NAMESPACE,
            "mis_anim.zbd",
            AssetVariant::default().as_str(),
        )
        .expect("a valid mission key");
        let asset = session
            .resolve(&key)
            .unwrap_or_else(|error| panic!("{} resolves: {error}", mission.directory.as_str()));
        assert_eq!(
            asset.resolved().span.container_path().to_lowercase(),
            mission.directory.logical_key(),
            "the bytes come from that mission's own directory"
        );
        assert_eq!(
            session.read_all(&asset).expect("the archive is read"),
            fs::read(root.join(mission.directory.as_str()).join("mis_anim.zbd"))
                .expect("the host file is read"),
            "{} reads back exactly the host file",
            mission.directory.as_str()
        );
        checked += 1;
    }
    assert!(
        checked > 1,
        "more than one mission directory is checked, so the loop is real: {checked}"
    );
}

/// A retail context selecting `mission`'s world group and mission.
fn context_for(mission: &MissionDirectory) -> ResolveContext {
    ResolveContext::new(fingerprint_of_retail())
        .with_world_group(mission.world())
        .with_mission(
            mission
                .scope()
                .unwrap_or_else(|error| panic!("{} is a valid label: {error}", mission.directory)),
        )
}

/// The installation's installation fingerprint, from the discovered manifest.
fn fingerprint_of_retail() -> ContentHash {
    static FINGERPRINT: std::sync::OnceLock<ContentHash> = std::sync::OnceLock::new();
    *FINGERPRINT.get_or_init(|| install::fingerprint(&retail_discovery().manifest))
}

fn retail_discovery() -> &'static install::Discovery {
    static DISCOVERY: std::sync::OnceLock<install::Discovery> = std::sync::OnceLock::new();
    DISCOVERY.get_or_init(|| {
        install::discover(&retail_root())
            .unwrap_or_else(|error| panic!("the installation is discovered: {error}"))
    })
}

// ------------------------------------------------------- evidence harness ---

/// What the recorded acceptance run reported about the task's own tests.
#[derive(Debug, Default)]
struct Suite {
    /// Tests that ran and passed.
    passed: usize,
    /// Tests that ran and failed.
    failed: usize,
    /// Tests that were discovered but ignored (they were not run).
    ignored: usize,
    /// Tests that ran at all.
    executed: usize,
    /// Tests the run discovered.
    discovered: usize,
    /// Each task test and its status, `pass` or `fail`.
    assertions: Vec<(String, &'static str)>,
}

/// The `N ignored` of one `test result:` summary line.
fn ignored_count(summary: &str) -> Option<usize> {
    summary.split(';').find_map(|segment| {
        let words: Vec<&str> = segment.split_whitespace().collect();
        words
            .windows(2)
            .find_map(|pair| (pair[1] == "ignored").then(|| pair[0].parse::<usize>().ok())?)
    })
}

fn record(suite: &mut Suite, name: &str, status: &'static str) {
    if status == "pass" {
        suite.passed += 1;
    } else {
        suite.failed += 1;
    }
    suite.assertions.push((name.to_owned(), status));
}

/// Reads the task-prefixed results out of the recorded run's log.
///
/// Both cargo layouts are understood: the `test <name> ... ok` lines of a
/// default run and the same lines **wrapped onto the next line** when a test
/// takes longer than the harness's progress threshold (this suite's retail
/// tests do), plus the `test result:` summaries, which is where the ignored
/// tests come from — a run without `--include-ignored` lists them as ignored.
fn parse_suite(log: &str) -> Suite {
    let mut suite = Suite::default();
    let mut pending: VecDeque<String> = VecDeque::new();
    for line in log.lines() {
        let trimmed = line.trim_start();
        // The `test result:` summaries are read for the **ignored** count only:
        // a run without `--include-ignored` lists a task test there and nowhere
        // else, and the executed counts come from the per-test lines, which
        // would otherwise be counted twice.
        if let Some(summary) = trimmed.strip_prefix("test result:") {
            if let Some(count) = ignored_count(summary) {
                suite.ignored += count;
            }
            continue;
        }
        if !pending.is_empty() && (trimmed == "ok" || trimmed == "FAILED") {
            let name = pending.pop_front().expect("a pending task test");
            record(
                &mut suite,
                &name,
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
            cursor = &after[separator + 5..];
            if !name.starts_with(PREFIX) {
                continue;
            }
            match cursor.split_whitespace().next() {
                Some("ok") => record(&mut suite, &name, "pass"),
                Some("FAILED") => record(&mut suite, &name, "fail"),
                _ => pending.push_back(name),
            }
        }
    }
    suite.executed = suite.passed + suite.failed;
    suite.discovered = suite.assertions.len() + suite.ignored;
    suite
}

/// Evidence-report harness for task #687 (`docs/contracts/CLI-EVIDENCE.md`,
/// schema `schemas/evidence.schema.json`). Not an acceptance test: it fails
/// loudly when its inputs are missing. `CS_EVIDENCE_REVIEWER` names the agent
/// that ran it and is recorded in the report; it is not baked in, because the
/// reviewer regenerates the report on the rebased commit. Run from the
/// workspace root, after the acceptance suite, exactly as:
///
/// 1. ```sh
///    mkdir -p private/evidence/T687
///    cargo test --workspace --locked -- accept_f04_d_order_archives_ --include-ignored \
///      2>&1 | tee private/evidence/T687/cargo-test.log
///    ```
///    (record the exit status of `cargo test`, e.g. with `pipefail`.)
/// 2. ```sh
///    CS_EVIDENCE_DIR=private/evidence/T687 \
///    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
///    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f04_d_order_archives_ --include-ignored" \
///    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
///    CS_EVIDENCE_REVIEWER="<agent running this harness>" \
///      cargo test --locked -p cs_assets --test accept_f04_d_order_archives -- evidence_report_t687 --ignored
///    ```
/// 3. ```sh
///    python3 tools/validate_evidence.py private/evidence/T687/acceptance.json \
///      --artifact-root private/evidence/T687
///    ```
///    **Without** `--require-pass`: that flag rejects a report that still lists
///    unresolved issues, and this task's `unknowns` are exactly its
///    limitations — the code-derived-only binding order, which texture tier a
///    world binds (#352's rule, unwired from here), and the reader loose-file
///    override (#700). They are recorded rather than dropped, so the flag
///    exits 3 with "Unresolved issues" and that is the expected result.
/// 4. Commit a copy of `acceptance.json` as
///    `docs/findings/evidence/T687.json`.
///
/// Every field is derived from real inputs: the recorded log, production
/// discovery of `$CS_GAME_DIR`, the production binding survey of every world
/// group and mission directory (`bindings.json`: installation spellings,
/// levels and the archives each load binds), `rustc --version` and
/// `Cargo.lock`. The `unknowns` are the literal limitations of
/// `docs/findings/2026-10-06-f04-d-archive-bindings.md`.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_t687_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(!argv.is_empty(), "CS_EVIDENCE_ARGV must hold the command");
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let reviewer = env_var("CS_EVIDENCE_REVIEWER");
    assert_eq!(
        candidate_tree,
        git(&["rev-parse", "HEAD^{tree}"]),
        "CS_CANDIDATE_TREE must be the tree that was tested; old reports cannot be reused for \
         new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", log_path.display()));
    let suite = parse_suite(&log);
    assert_eq!(
        suite.assertions.len(),
        TASK_TESTS,
        "every task test of this suite must appear exactly once in {}: {suite:?}",
        log_path.display()
    );
    assert_eq!(
        suite.failed, 0,
        "every task test must have passed: {suite:?}",
    );
    for retail_test in [
        format!("{PREFIX}retail_world_groups_bind_the_measured_archives"),
        format!("{PREFIX}retail_gamez_is_never_per_mission_and_mis_anim_always_is"),
        format!("{PREFIX}retail_each_world_searches_its_own_directory_then_the_shared_one"),
        format!("{PREFIX}retail_the_mission_level_mounts_every_mission_directory"),
    ] {
        assert_eq!(
            suite
                .assertions
                .iter()
                .find(|(name, _)| *name == retail_test)
                .map(|(_, status)| *status),
            Some("pass"),
            "{retail_test} must have run and passed (step 1 needs --include-ignored and \
             CS_GAME_DIR)"
        );
    }
    assert!(
        suite
            .assertions
            .iter()
            .any(|(name, _)| !name.contains("_retail_")),
        "synthetic task tests must be present alongside the retail ones"
    );

    // The survey is production work: it derives the bindings of every world
    // group and mission directory the installation ships.
    let survey = binding_survey();
    let world_rows = survey.iter().filter(|row| row.mission.is_none()).count();
    assert_eq!(
        world_rows,
        retail_diagnosis().world_groups.len(),
        "one world-level row per world group the installation ships"
    );
    assert_eq!(
        survey.len() - world_rows,
        mission_directories(retail_diagnosis()).len(),
        "one mission row per mission directory the installation ships"
    );
    for row in &survey {
        assert!(
            row.bindings
                .iter()
                .any(|binding| binding.starts_with("gamez=")),
            "{} binds its own gamez.zbd",
            row.mission.as_deref().unwrap_or(&row.world)
        );
    }
    let bindings_path = evidence_dir.join("bindings.json");
    fs::write(&bindings_path, bindings_json(&candidate_tree, &survey))
        .unwrap_or_else(|error| panic!("write {}: {error}", bindings_path.display()));
    let artifacts = [artifact(&log_path, "log"), artifact(&bindings_path, "json")];

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"F04-D-order-archives\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {{\"rust\": {}, \"bevy\": {}, \"avian\": {}}},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": [{}], \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [{}],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        json(&candidate_tree),
        json(&rustc_version()),
        json(&pinned("bevy")),
        json(&pinned("avian3d")),
        json(&now()),
        argv.iter()
            .map(|word| json(word))
            .collect::<Vec<_>>()
            .join(", "),
        json(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        json(&fingerprints(&retail_root()).0),
        json(&fingerprints(&retail_root()).1),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        suite
            .assertions
            .iter()
            .map(|(name, status)| format!(
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\"]}}",
                json(name)
            ))
            .collect::<Vec<_>>()
            .join(", "),
        artifacts
            .iter()
            .map(|entry| format!(
                "{{\"path\": {}, \"sha256\": {:?}, \"kind\": {:?}}}",
                entry.path, entry.sha256, entry.kind
            ))
            .collect::<Vec<_>>()
            .join(", "),
        UNKNOWNS
            .iter()
            .map(|unknown| json(unknown))
            .collect::<Vec<_>>()
            .join(", "),
        json(&reviewer),
        json(METHOD),
    );
    let report_path = evidence_dir.join("acceptance.json");
    fs::write(&report_path, &report)
        .unwrap_or_else(|error| panic!("write {}: {error}", report_path.display()));
    println!("wrote {}", report_path.display());
}

/// The literal limitations this stage left open, with the affected content and
/// the task that owns each (the machine-readable form of the finding's
/// "Recorded unknowns").
const UNKNOWNS: [&str; 4] = [
    "The archive bindings themselves are code-derived: gamez.zbd per world group, planes.zbd from the ZBD root, cam_anim.zbd then mis_anim.zbd, the one-texture-archive rule and the absence of a directory fallback were read off the original executable's INTERP scripts and loader code, never observed in a run of the original. BINDING_ORDER_STATUS is therefore `inferred` and PRECEDENCE_ORDER_STATUS stays `designed`. Affected content: every world and mission load's archive set. Settling it against a run of the original needs owner-supplied capture (REF-OWNER-FIRST-CAPTURE, #358) or further static work.",
    "Which texture tier a world binds is not decided here: this task records the search (the world's own directory, then the shared ZBD directory) and holds the file the measured rule selected, refusing a second. The rule itself (texture budget, the descending rtextureN/textureN walk, the software renderer's texture.zbd) is task #352 in cs_content::textures, which depends on this crate and so cannot be called from it, and it is not wired into a world load. Affected content: every world's texture archive. Resolving tasks: #352, #688.",
    "The original's loose-file override and loose-directory fallback are not modelled: a loose file of the same basename that is newer wins over an archive member, and the loose directories are searched only when no archive holds the name. Affected content: every reader (.zrd) member resolved through cs_assets::vfs::reader. Resolving task: #700; the index entry's unexplained bytes, which are the only archive-side time candidate, stay #692.",
    "Which archives a mission that is not a directory of a world group would use is not settled: the original's mission-name tables are campaign-type dependent (m01..m05, mp1..mp5, ia1) and this task derives the mission level from the directories the installation has, so a mission with no directory binds nothing beyond its world's archives. Affected content: any mission not shipped as ZBD/<world group>/<mission>. Resolving task: the campaign/mission task that owns the mission table.",
];

/// One world group or mission directory and the archives its load binds.
struct SurveyRow {
    /// The world group's installation spelling.
    world: String,
    /// The mission directory's spelling, or `None` for a world-level row.
    mission: Option<String>,
    /// The bound archives, as `role=container` rows in script order.
    bindings: Vec<String>,
}

/// Derives every row through production code: discovery, then
/// [`WorldLayout::for_context`].
fn binding_survey() -> Vec<SurveyRow> {
    let diagnosis = retail_diagnosis();
    let shared = retail_shared();
    let mut rows = Vec::new();
    for group in &diagnosis.world_groups {
        let world = WorldGroup::from_relative(group.clone());
        let layout = WorldLayout::for_context(
            &ResolveContext::new(sha256(&[])).with_world_group(world.clone()),
            &shared,
        )
        .unwrap_or_else(|error| panic!("{} binds its archives: {error}", group.as_str()));
        rows.push(SurveyRow {
            world: group.as_str().to_owned(),
            mission: None,
            bindings: layout
                .archives()
                .iter()
                .map(|archive| {
                    format!(
                        "{}={} ({})",
                        archive.role().label(),
                        archive.container().as_str(),
                        archive.level().label()
                    )
                })
                .collect(),
        });
        for mission in mission_directories(diagnosis)
            .into_iter()
            .filter(|mission| mission.world_group == *group)
        {
            let mission_layout = WorldLayout::for_context(
                &ResolveContext::new(sha256(&[]))
                    .with_world_group(world.clone())
                    .with_mission(
                        mission
                            .scope()
                            .unwrap_or_else(|error| panic!("{} is a label: {error}", mission)),
                    ),
                &shared,
            )
            .unwrap_or_else(|error| {
                panic!("{} binds its archives: {error}", mission.directory.as_str())
            });
            rows.push(SurveyRow {
                world: group.as_str().to_owned(),
                mission: Some(mission.directory.as_str().to_owned()),
                bindings: mission_layout
                    .archives()
                    .iter()
                    .map(|archive| {
                        format!(
                            "{}={} ({})",
                            archive.role().label(),
                            archive.container().as_str(),
                            archive.level().label()
                        )
                    })
                    .collect(),
            });
        }
    }
    rows
}

fn bindings_json(candidate_tree: &str, rows: &[SurveyRow]) -> String {
    let rendered: Vec<String> = rows
        .iter()
        .map(|row| {
            let bindings: Vec<String> = row.bindings.iter().map(|binding| json(binding)).collect();
            format!(
                "\n    {{\"world\": {}, \"mission\": {}, \"bindings\": [{}]}}",
                json(&row.world),
                row.mission.as_deref().map_or("null".to_owned(), json),
                bindings.join(", ")
            )
        })
        .collect();
    format!(
        "{{\n  \"task\": \"F04-D-order-archives\",\n  \"candidate_tree\": {},\n  \
         \"order_status\": {},\n  \"rows\": [{}]\n}}\n",
        json(candidate_tree),
        json(BINDING_ORDER_STATUS.label()),
        rendered.join(",")
    )
}

// --------------------------------------------------------- harness helpers ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|error| panic!("{name} must be set: {error}"))
}

/// The workspace root: this test binary runs under `target/`.
fn workspace_path(relative: &str) -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(Path::parent)
        .expect("the crate is inside the workspace")
        .join(relative)
}

fn git(args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(workspace_path("."))
        .output()
        .unwrap_or_else(|error| panic!("git {:?}: {error}", args));
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git prints utf-8")
        .trim()
        .to_owned()
}

fn rustc_version() -> String {
    let output = Command::new("rustc")
        .arg("--version")
        .output()
        .expect("rustc runs");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// One dependency's pinned version out of the workspace `Cargo.lock`, under
/// the name the lock file spells it with.
fn pinned(crate_name: &str) -> String {
    let lock = fs::read_to_string(workspace_path("Cargo.lock")).expect("Cargo.lock is readable");
    let mut lines = lock.lines();
    while let Some(line) = lines.next() {
        if line == format!("name = \"{crate_name}\"") {
            let version = lines
                .find(|line| line.trim_start().starts_with("version = "))
                .unwrap_or_else(|| panic!("{crate_name} has a version in Cargo.lock"));
            return version
                .trim()
                .trim_start_matches("version = ")
                .trim_matches('"')
                .to_owned();
        }
    }
    panic!("{crate_name} is not in Cargo.lock")
}

/// The installation's installation and content fingerprints, from production
/// discovery of the same manifest the F02 evidence reports use.
fn fingerprints(root: &Path) -> (String, String) {
    let found = install::discover(root).expect("the installation is discovered");
    (
        install::fingerprint(&found.manifest).to_string(),
        install::content_fingerprint(&found.manifest).to_string(),
    )
}

fn now() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock is after the Unix epoch")
        .as_secs();
    // RFC 3339 in UTC, from the epoch seconds.
    let days = seconds / 86_400;
    let rest = seconds % 86_400;
    let (year, month, day) = civil_from_days(days as i64);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        (rest % 3_600) / 60,
        rest % 60
    )
}

/// Days since the Unix epoch to a civil date (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

fn json(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// One artifact of the evidence report: its name, kind and SHA-256.
struct Artifact {
    path: String,
    kind: &'static str,
    sha256: String,
}

fn artifact(path: &Path, kind: &'static str) -> Artifact {
    let bytes = fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    Artifact {
        path: path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string()),
        kind,
        sha256: sha256(&bytes).to_string(),
    }
}

/// What this harness did, for the report's `review.method`.
const METHOD: &str = "acceptance suite run locally with the retail capability; this harness derives \
every field from the recorded log, production discovery of $CS_GAME_DIR, the production \
binding survey of all eight world groups and their 53 mission directories, rustc and Cargo.lock. \
The archive bindings, their levels and their order are read off the original executable's code, \
so BINDING_ORDER_STATUS is `inferred` and PRECEDENCE_ORDER_STATUS stays `designed`. The \
`unknowns` array is deliberately non-empty and names the affected content and its resolving task \
for each limitation, so this report must be validated WITHOUT --require-pass: that flag rejects a \
report with unresolved issues and would only be green if they had been dropped. The claim is \
`implemented`, never `verified_original` or `release_approved`.";
