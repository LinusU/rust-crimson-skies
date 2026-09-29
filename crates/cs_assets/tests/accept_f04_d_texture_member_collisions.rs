//! Member-level collisions between the per-world ZBD texture archives
//! (task #346, F04-D member collisions, follow-up 2 in
//! `docs/findings/2026-09-28-f04-d-observed-collisions-and-async-cancel.md`).
//!
//! F04-D compared collisions between *files*. The retail installation stores
//! six texture archives per world group (`ZBD/C1/texture.zbd` and five
//! `ZBD/C1/rtexture*.zbd`), and the same texture name is stored in many of
//! them: a first-wins basename map — what the community tool S03 does — would
//! serve one world's texture to another, which spec F04 non-negotiable
//! behavior 3 forbids. Those overlaps only exist once the archives' members
//! are mounted, which is what this test does: every retail `texture.zbd` /
//! `rtexture*.zbd` member index is mounted as a **retail** mount bound to its
//! world group (a designed scope), and `ContentSession::collision_report`
//! runs under no world and under every world group.
//!
//! Which archive a world really uses — `texture.zbd` or one of the
//! `rtexture*.zbd` tiers, and in which order a name is looked up when several
//! hold it — is **not measured** (the F08-C recorded unknown; #352 owns
//! establishing it). The designed baseline ranks the world's `texture.zbd`
//! ([`PrecedenceClass::MissionWorld`]) above its `rtexture*.zbd`
//! ([`PrecedenceClass::Shared`]) because F02-C observed `texture.zbd` as the
//! expected primary archive of a group while the `rtexture*.zbd` names are
//! inventoried but not expected. That preference is only `designed`, so every
//! retail lookup it would decide between different bytes is refused with
//! [`ResolveError::UnmeasuredOrder`] (spec F04 non-negotiable behavior 2).
//! Nothing here claims the original prefers `texture.zbd`.
//!
//! The synthetic tests author ZBD texture packages in memory and mount their
//! members through the production `read_zbd_textures` + `MountBuilder`. The
//! `retail_` test reads `$CS_GAME_DIR` read-only and fails loudly without it;
//! CI skips it. `evidence_report_t346_writes_the_acceptance_report` is the
//! evidence harness (`docs/contracts/CLI-EVIDENCE.md`), not an acceptance
//! test.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{self, Discovery, content_fingerprint, fingerprint, sha256};
use cs_assets::vfs::{
    CollisionReport, CollisionVerdict, ContentSession, LookupOutcome, Mount, MountBuilder,
    ResolveError, SessionBuilder,
};
use cs_formats::io::AllocationBudget;
use cs_formats::texture::read_zbd_textures;
use cs_formats::texture::zbd::{
    FLAG_BYTES_PER_PIXEL2, FLAG_NO_ALPHA, ZBD_TEXTURE_ENTRY_BYTES, ZBD_TEXTURE_HEADER_BYTES,
};
use cs_types::asset_id::{
    AssetKey, MountId, MountNamespace, PrecedenceClass, ResolveContext, WorldGroup,
};
use cs_types::evidence::{ClaimStatus, ContentHash};

/// The acceptance prefix of this task.
const PREFIX: &str = "accept_f04_d_texture_member_collisions_";

/// The key space every texture archive is mounted in: an engine-authored
/// label, not an observed retail namespace.
const NAMESPACE: &str = "texture";

/// The world's primary texture archive name.
const TEXTURE_ZBD: &str = "texture.zbd";

/// `NO_ALPHA | BYTES_PER_PIXEL2`: what a retail direct-color texture stores.
const OPAQUE: u32 = FLAG_BYTES_PER_PIXEL2 | FLAG_NO_ALPHA;

// ------------------------------------------------------------ fixtures ---

/// A minimal valid ZBD texture package: one 1x1 opaque RGB565 texture per
/// `(name, word)`, authored byte by byte from the layout of
/// `docs/findings/2026-09-28-f08-b-02-zbd-texture-package.md`. Two packages
/// with different words store different bytes under the same name.
fn package(textures: &[(&str, u16)]) -> Vec<u8> {
    assert!(!textures.is_empty(), "a package holds at least one texture");
    let mut out = Vec::new();
    for word in [0u32, 1, 0, textures.len() as u32, 0, 0] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    let mut offset = ZBD_TEXTURE_HEADER_BYTES + textures.len() * ZBD_TEXTURE_ENTRY_BYTES;
    let bodies: Vec<Vec<u8>> = textures
        .iter()
        .map(|(_, word)| {
            let mut body = Vec::new();
            body.extend_from_slice(&OPAQUE.to_le_bytes());
            body.extend_from_slice(&1u16.to_le_bytes());
            body.extend_from_slice(&1u16.to_le_bytes());
            body.extend_from_slice(&0u32.to_le_bytes());
            body.extend_from_slice(&0u16.to_le_bytes());
            body.extend_from_slice(&0u16.to_le_bytes());
            body.extend_from_slice(&word.to_le_bytes());
            body
        })
        .collect();
    for ((name, _), body) in textures.iter().zip(&bodies) {
        let mut field = [0u8; 32];
        assert!(name.len() < field.len(), "{name:?} fits the name field");
        field[..name.len()].copy_from_slice(name.as_bytes());
        out.extend_from_slice(&field);
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        out.extend_from_slice(&(-1i32).to_le_bytes());
        offset += body.len();
    }
    for body in bodies {
        out.extend_from_slice(&body);
    }
    out
}

/// A mount id label derived from a container spelling.
fn mount_id(container: &str) -> MountId {
    let label: String = container
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '.' {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    MountId::new(&label).expect("a valid mount id label")
}

/// Mounts every member of the ZBD package `bytes` as a retail mount, bound to
/// `world` when one is given.
///
/// There is no production texture mounter yet (`cs_content::textures` reads a
/// single archive through a session; it does not build a member index), so
/// this mounts the index the production reader exposes: the stored level of
/// each texture, its offset inside the container and its digest.
fn archive_mount(
    container: &str,
    precedence: PrecedenceClass,
    world: Option<&WorldGroup>,
    bytes: &[u8],
) -> Mount {
    let mut budget = AllocationBudget::with_defaults(container);
    let package =
        read_zbd_textures(container, bytes, &mut budget).expect("a valid ZBD texture package");
    let base = bytes.as_ptr() as usize;
    let mut builder = MountBuilder::new(
        mount_id(container),
        MountNamespace::new(NAMESPACE).expect("a valid namespace"),
        precedence,
        container,
    )
    .retail();
    if let Some(world) = world {
        builder = builder.with_world_group(world.clone());
    }
    for texture in package.textures() {
        let offset = texture.stored().as_ptr() as usize - base;
        let digest = sha256(texture.stored());
        builder
            .add_member(
                texture.name(),
                texture.stored().len() as u64,
                offset as u64,
                Some(digest),
            )
            .unwrap_or_else(|error| panic!("{container}: texture {:?}: {error}", texture.name()));
    }
    builder.build().expect("the mount builds")
}

fn key(spelling: &str) -> AssetKey {
    AssetKey::from_spelling(NAMESPACE, spelling, "default").expect("the key is valid")
}

/// No world plus each of `worlds`.
fn contexts(base: &ResolveContext, worlds: &[WorldGroup]) -> Vec<ResolveContext> {
    let mut contexts = vec![base.clone()];
    contexts.extend(
        worlds
            .iter()
            .map(|world| base.clone().with_world_group(world.clone())),
    );
    contexts
}

fn synthetic_worlds() -> Vec<WorldGroup> {
    ["zbd/c1", "zbd/c2"]
        .iter()
        .map(|world| WorldGroup::new(world).expect("the world is valid"))
        .collect()
}

/// One session with `mounts`, under `context`.
fn session_with(context: ResolveContext, mounts: &[Mount]) -> ContentSession {
    let mut builder = SessionBuilder::new(context);
    for mount in mounts {
        builder.mount(mount.clone()).expect("a unique mount id");
    }
    builder.open()
}

// ----------------------------------------------------------- synthetic ---

/// AC01: two worlds hold a same-named texture with different bytes. Each
/// world-bound retail mount serves its own member and never the other
/// world's; under no world nothing is served. The shared name is a collision
/// only of names: nothing reaches the precedence order, so the verdict is
/// `distinct_by_path` — not a first-wins flattening.
#[test]
fn accept_f04_d_texture_member_collisions_each_world_serves_its_own() {
    let worlds = synthetic_worlds();
    let world_a = &worlds[0];
    let world_b = &worlds[1];
    let a_bytes = package(&[("SHARED_TEX", 0xF800), ("A_ONLY", 0x07E0)]);
    let b_bytes = package(&[("SHARED_TEX", 0x001F), ("B_ONLY", 0xFFE0)]);
    let a = archive_mount(
        "ZBD/C1/texture.zbd",
        PrecedenceClass::MissionWorld,
        Some(world_a),
        &a_bytes,
    );
    let b = archive_mount(
        "ZBD/C2/texture.zbd",
        PrecedenceClass::MissionWorld,
        Some(world_b),
        &b_bytes,
    );
    let (a_id, b_id) = (a.id().clone(), b.id().clone());
    let a_shared = a.member(&key("SHARED_TEX")).expect("A holds it").sha256();
    let b_shared = b.member(&key("SHARED_TEX")).expect("B holds it").sha256();
    assert!(a_shared.is_some() && b_shared.is_some() && a_shared != b_shared);

    let context = ResolveContext::new(sha256(b"t346 synthetic worlds"));
    let no_world = session_with(context.clone(), std::slice::from_ref(&a));
    let session_a = session_with(
        context.clone().with_world_group(world_a.clone()),
        &[a.clone(), b.clone()],
    );
    let session_b = session_with(
        context.clone().with_world_group(world_b.clone()),
        &[a.clone(), b.clone()],
    );

    // Under world A only A is eligible; under world B only B; with no world
    // neither is.
    let resolved_a = session_a.resolve(&key("SHARED_TEX")).expect("A serves it");
    assert_eq!(resolved_a.resolved().mount, a_id);
    assert_eq!(resolved_a.resolved().span.member_sha256(), a_shared);
    assert_eq!(
        resolved_a.resolved().span.container_path(),
        "ZBD/C1/texture.zbd"
    );
    let resolved_b = session_b.resolve(&key("SHARED_TEX")).expect("B serves it");
    assert_eq!(resolved_b.resolved().mount, b_id);
    assert_eq!(resolved_b.resolved().span.member_sha256(), b_shared);
    assert_eq!(
        resolved_b.resolved().span.container_path(),
        "ZBD/C2/texture.zbd"
    );
    assert!(matches!(
        session_a.resolve(&key("B_ONLY")),
        Err(ResolveError::NotFound { .. })
    ));
    assert!(matches!(
        no_world.resolve(&key("SHARED_TEX")),
        Err(ResolveError::NotFound { .. })
    ));

    let report = session_a.collision_report(&contexts(&context, &worlds));
    assert_eq!(report.precedence_status, ClaimStatus::Designed);
    assert_eq!(report.contexts.len(), 3);
    assert_eq!(report.comparisons.len(), 1, "only the shared name collides");
    let comparison = &report.comparisons[0];
    assert_eq!(comparison.collision.file_name, "shared_tex");
    assert_eq!(comparison.collision.members.len(), 2);
    assert_eq!(comparison.collision.distinct_digests(), 2);
    assert_eq!(comparison.verdict, CollisionVerdict::DistinctByPath);
    assert_eq!(comparison.lookups.len(), 2 * 3);
    for lookup in &comparison.lookups {
        let member = &comparison.collision.members[lookup.member];
        let world = report.contexts[lookup.context].world_group.clone();
        let own = match world.as_ref() {
            Some(world) if *world == *world_a => member.container == "ZBD/C1/texture.zbd",
            Some(world) if *world == *world_b => member.container == "ZBD/C2/texture.zbd",
            _ => false,
        };
        assert_eq!(
            lookup.outcome,
            if own {
                LookupOutcome::Own
            } else {
                LookupOutcome::NotEligible
            },
            "{} under {}",
            member.container,
            world.map_or_else(|| "no world".to_owned(), |world| world.to_string())
        );
    }
    assert_eq!(report.conflicting().count(), 0);
}

/// A shared-scope overlap: two original (retail) mounts serve every world and
/// hold one name with different bytes. The designed order (patch over shared)
/// would prefer one, but nothing measured says the original does, so the
/// lookup is refused with `UnmeasuredOrder` under every context — through the
/// session's own `resolve` as well as the collision report.
#[test]
fn accept_f04_d_texture_member_collisions_shared_scope_overlap_is_blocked() {
    let base = archive_mount(
        "synthetic/base.zbd",
        PrecedenceClass::Shared,
        None,
        &package(&[("SHARED_TEX", 0x1111)]),
    );
    let overlay = archive_mount(
        "synthetic/overlay.zbd",
        PrecedenceClass::Patch,
        None,
        &package(&[("SHARED_TEX", 0x2222)]),
    );
    let (base_id, overlay_id) = (base.id().clone(), overlay.id().clone());
    let context = ResolveContext::new(sha256(b"t346 shared scope"));
    let session = session_with(context.clone(), &[base, overlay]);
    let worlds = synthetic_worlds();

    match session.resolve(&key("SHARED_TEX")) {
        Err(ResolveError::UnmeasuredOrder {
            selected, shadowed, ..
        }) => {
            assert_eq!(selected.mount, overlay_id);
            assert_eq!(selected.container, "synthetic/overlay.zbd");
            assert_eq!(shadowed.len(), 1);
            assert_eq!(shadowed[0].mount, base_id);
            assert_eq!(shadowed[0].container, "synthetic/base.zbd");
        }
        other => panic!("the overlay must not win by the designed order alone: {other:?}"),
    }

    let report = session.collision_report(&contexts(&context, &worlds));
    assert_eq!(report.precedence_status, ClaimStatus::Designed);
    assert_eq!(report.comparisons.len(), 1);
    let comparison = &report.comparisons[0];
    assert_eq!(comparison.verdict, CollisionVerdict::Conflicting);
    assert_eq!(comparison.collision.distinct_digests(), 2);
    assert_eq!(comparison.lookups.len(), 2 * 3);
    for lookup in &comparison.lookups {
        match &lookup.outcome {
            LookupOutcome::Blocked { selected, shadowed } => {
                assert_eq!(selected.mount, overlay_id);
                assert_eq!(shadowed.len(), 1);
                assert_eq!(shadowed[0].mount, base_id);
                assert_ne!(shadowed[0].sha256, selected.sha256);
            }
            other => panic!(
                "{} of {}: expected blocked, got {other}",
                comparison.collision.members[lookup.member].spelling,
                comparison.collision.members[lookup.member].container
            ),
        }
    }
    assert_eq!(report.conflicting().count(), 1);
}

/// Non-negotiable behavior 3: two retail mounts at *equal* priority hold one
/// name with different bytes. The answer is `Ambiguous` carrying both
/// origins, never a first-wins pick.
#[test]
fn accept_f04_d_texture_member_collisions_equal_priority_overlap_is_ambiguous() {
    let one = archive_mount(
        "synthetic/one.zbd",
        PrecedenceClass::Shared,
        None,
        &package(&[("SHARED_TEX", 0x3333)]),
    );
    let two = archive_mount(
        "synthetic/two.zbd",
        PrecedenceClass::Shared,
        None,
        &package(&[("SHARED_TEX", 0x4444)]),
    );
    let (one_id, two_id) = (one.id().clone(), two.id().clone());
    let context = ResolveContext::new(sha256(b"t346 equal priority"));
    let session = session_with(context.clone(), &[one, two]);

    match session.resolve(&key("SHARED_TEX")) {
        Err(ResolveError::Ambiguous { candidates, .. }) => {
            let mounts: BTreeSet<MountId> = candidates
                .iter()
                .map(|origin| origin.mount.clone())
                .collect();
            assert_eq!(mounts, BTreeSet::from([one_id.clone(), two_id.clone()]));
            assert!(
                candidates
                    .iter()
                    .all(|origin| origin.precedence == PrecedenceClass::Shared)
            );
        }
        other => panic!("equal-priority origins must stay ambiguous: {other:?}"),
    }

    let report = session.collision_report(&contexts(&context, &synthetic_worlds()));
    let comparison = &report.comparisons[0];
    assert_eq!(comparison.verdict, CollisionVerdict::Conflicting);
    for lookup in &comparison.lookups {
        match &lookup.outcome {
            LookupOutcome::Ambiguous(candidates) => assert_eq!(candidates.len(), 2),
            other => panic!("expected ambiguous, got {other}"),
        }
    }
}

// ------------------------------------------------------------- retail ---

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var_os("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

/// Discovery hashes the whole installation; share one run between the retail
/// test and the evidence harness of this binary.
fn discovery() -> &'static Discovery {
    static DISCOVERY: OnceLock<Discovery> = OnceLock::new();
    DISCOVERY
        .get_or_init(|| install::discover(&game_dir()).expect("the installation is discovered"))
}

/// One mounted retail texture archive.
struct Archive {
    /// The world group's logical key, e.g. `zbd/c1`.
    world_key: String,
    /// The installation-relative archive path, e.g. `ZBD/C1/texture.zbd`.
    container: String,
    /// Whether this is the world's `texture.zbd` (the designed primary).
    primary: bool,
    /// The id the mount is registered under.
    mount_id: MountId,
    /// How many members the mount holds.
    member_count: usize,
    /// Every member name and its stored-level digest.
    digests: BTreeMap<String, ContentHash>,
}

/// The retail installation, every texture archive mounted, and its collision
/// report across no world and every world group.
struct Retail {
    install_sha256: String,
    content_sha256: String,
    session: ContentSession,
    report: CollisionReport,
    archives: Vec<Archive>,
}

fn retail() -> Retail {
    let root = game_dir();
    let found = discovery();
    let worlds: Vec<(WorldGroup, String)> = found
        .diagnosis
        .world_groups
        .iter()
        .map(|group| {
            (
                WorldGroup::from_relative(group.clone()),
                group.as_str().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        worlds.len(),
        8,
        "the reference installation has eight world groups"
    );

    let install_context = ResolveContext::new(fingerprint(&found.manifest));
    // The session context selects the first world group (`zbd/c1`); the
    // collision report compares every world explicitly.
    let mut builder = SessionBuilder::new(
        install_context
            .clone()
            .with_world_group(worlds[0].0.clone()),
    );
    let mut archives = Vec::new();
    for (group, spelling) in &worlds {
        let mut dir = root.clone();
        dir.extend(spelling.split(['/', '\\']));
        let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
            .unwrap_or_else(|error| panic!("read {}: {error}", dir.display()))
            .map(|entry| entry.expect("a readable entry").path())
            .collect();
        paths.sort();
        for path in paths {
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .expect("a UTF-8 file name")
                .to_owned();
            let lower = file_name.to_ascii_lowercase();
            let primary = lower == TEXTURE_ZBD;
            let tier = lower.starts_with("rtexture") && lower.ends_with(".zbd");
            if !primary && !tier {
                continue;
            }
            let container = format!("{spelling}/{file_name}");
            let bytes =
                fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
            let precedence = if primary {
                PrecedenceClass::MissionWorld
            } else {
                PrecedenceClass::Shared
            };
            let mount = archive_mount(&container, precedence, Some(group), &bytes);
            let mount_id = mount.id().clone();
            let member_count = mount.member_count();
            let mut digests = BTreeMap::new();
            for (_, member) in mount.members() {
                let digest = member.sha256().expect("every mounted member is hashed");
                assert!(
                    digests
                        .insert(member.spelling().as_str().to_owned(), digest)
                        .is_none(),
                    "{container} stores a name twice"
                );
            }
            builder
                .mount(mount)
                .unwrap_or_else(|error| panic!("{container}: {error}"));
            archives.push(Archive {
                world_key: group.logical_key(),
                container,
                primary,
                mount_id,
                member_count,
                digests,
            });
        }
    }
    let session = builder.open();
    let report = session.collision_report(&contexts(
        &install_context,
        worlds
            .iter()
            .map(|(group, _)| group)
            .cloned()
            .collect::<Vec<_>>()
            .as_slice(),
    ));
    Retail {
        install_sha256: fingerprint(&found.manifest).to_hex(),
        content_sha256: content_fingerprint(&found.manifest).to_hex(),
        session,
        report,
        archives,
    }
}

/// Every archive that stores each texture name, computed from the mounted
/// members independently of the collision report.
fn holders(retail: &Retail) -> BTreeMap<String, BTreeSet<String>> {
    let mut holders: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for archive in &retail.archives {
        for name in archive.digests.keys() {
            holders
                .entry(name.clone())
                .or_default()
                .insert(archive.container.clone());
        }
    }
    holders
}

fn archive_of<'a>(retail: &'a Retail, container: &str) -> &'a Archive {
    retail
        .archives
        .iter()
        .find(|archive| archive.container == container)
        .unwrap_or_else(|| panic!("{container} is a mounted archive"))
}

/// Every retail texture name shared by more than one archive: each world
/// serves its own archives and never another world's. Within a world all six
/// archives hold the name with different bytes, so the designed order (that
/// world's `texture.zbd` over its five `rtexture*.zbd` tiers) alone would
/// decide it and *every* eligible lookup — the primary's included — is
/// refused with `UnmeasuredOrder` rather than served.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f04_d_texture_member_collisions_retail_each_world_keeps_its_textures() {
    let retail = retail();
    let install_sha256 = &retail.install_sha256;
    assert_eq!(retail.report.precedence_status, ClaimStatus::Designed);
    assert_eq!(retail.report.contexts.len(), 9);
    assert_eq!(retail.archives.len(), 48, "installation {install_sha256}");

    // The shape this task measured and recorded in
    // `docs/findings/2026-09-29-t346-texture-archive-member-collisions.md`.
    // It is pinned so a reader or mount change that mounts less than the
    // archives hold fails here instead of quietly invalidating the findings;
    // a different installation is a different measurement, and the message
    // names the fingerprint it was taken from.
    let mut per_world: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut world_names: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for archive in &retail.archives {
        let entry = per_world.entry(archive.world_key.clone()).or_default();
        entry.0 += 1;
        entry.1 += archive.member_count;
        world_names
            .entry(archive.world_key.clone())
            .or_default()
            .extend(archive.digests.keys().cloned());
    }
    let expected: BTreeMap<&str, (usize, usize)> = [
        ("zbd/c1", (6, 881)),
        ("zbd/c1b", (6, 667)),
        ("zbd/c1c", (6, 593)),
        ("zbd/c2", (6, 820)),
        ("zbd/c2b", (6, 601)),
        ("zbd/c3", (6, 732)),
        ("zbd/c4", (6, 935)),
        ("zbd/c5", (6, 896)),
    ]
    .into_iter()
    .collect();
    let observed: BTreeMap<&str, (usize, usize)> = per_world
        .iter()
        .map(|(world, (archives, _))| (world.as_str(), (*archives, world_names[world].len())))
        .collect();
    assert_eq!(observed, expected, "installation {install_sha256}");
    for world in world_names.keys() {
        let primaries = retail
            .archives
            .iter()
            .filter(|archive| archive.world_key == *world && archive.primary)
            .count();
        assert_eq!(primaries, 1, "{world} has exactly one {TEXTURE_ZBD}");
    }
    assert!(retail.archives.iter().any(|archive| archive.primary));

    // Every name appears in whole worlds, never in a subset of one world:
    // a name in a world is in all six of its archives, so each world's six
    // archives hold the same set.
    for archive in &retail.archives {
        assert_eq!(
            archive.digests.len(),
            world_names[&archive.world_key].len(),
            "{} holds a subset of {}",
            archive.container,
            archive.world_key
        );
    }

    let holders = holders(&retail);
    assert_eq!(holders.len(), 1676, "installation {install_sha256}");
    assert_eq!(
        holders.values().filter(|set| set.len() == 48).count(),
        551,
        "installation {install_sha256}"
    );
    // Names come in whole worlds: a name is held by six archives per world it
    // appears in, so its archive count is 6 × (worlds holding it). The
    // histogram is the distribution of worlds per name, and no name is held
    // by a single archive (which would be a subset of a world).
    let mut held_by: BTreeMap<usize, usize> = BTreeMap::new();
    for set in holders.values() {
        *held_by.entry(set.len()).or_default() += 1;
    }
    let expected_held_by: BTreeMap<usize, usize> = [
        (6, 839),
        (12, 148),
        (18, 57),
        (24, 25),
        (30, 28),
        (36, 25),
        (42, 3),
        (48, 551),
    ]
    .into_iter()
    .collect();
    assert_eq!(held_by, expected_held_by, "installation {install_sha256}");
    assert_eq!(
        holders.values().map(BTreeSet::len).sum::<usize>(),
        36750,
        "installation {install_sha256}"
    );

    let primary_of: BTreeMap<&str, &MountId> = retail
        .archives
        .iter()
        .filter(|archive| archive.primary)
        .map(|archive| (archive.world_key.as_str(), &archive.mount_id))
        .collect();
    let tiers_of: BTreeMap<&str, BTreeSet<&MountId>> = retail
        .archives
        .iter()
        .filter(|archive| !archive.primary)
        .fold(BTreeMap::new(), |mut map, archive| {
            map.entry(archive.world_key.as_str())
                .or_default()
                .insert(&archive.mount_id);
            map
        });
    for (world, tiers) in &tiers_of {
        assert_eq!(tiers.len(), 5, "{world} has five rtexture tiers");
    }

    assert_eq!(retail.report.comparisons.len(), holders.len());
    assert_eq!(retail.report.conflicting().count(), holders.len());
    for comparison in &retail.report.comparisons {
        let name = &comparison.collision.file_name;
        let here = &holders[name];
        let member_containers: BTreeSet<&str> = comparison
            .collision
            .members
            .iter()
            .map(|member| member.container.as_str())
            .collect();
        assert_eq!(
            member_containers,
            here.iter().map(String::as_str).collect::<BTreeSet<_>>(),
            "{name}"
        );
        let digests: BTreeSet<String> = here
            .iter()
            .map(|container| archive_of(&retail, container).digests[name].to_hex())
            .collect();
        assert_eq!(comparison.collision.distinct_digests(), digests.len());
        assert!(
            digests.len() >= 2,
            "{name} is held with one digest everywhere and is not a collision"
        );
        assert_eq!(comparison.verdict, CollisionVerdict::Conflicting, "{name}");
        for lookup in &comparison.lookups {
            let member = &comparison.collision.members[lookup.member];
            let archive = archive_of(&retail, &member.container);
            let context_world = retail.report.contexts[lookup.context]
                .world_group
                .as_ref()
                .map(WorldGroup::logical_key);
            if context_world.as_deref() != Some(archive.world_key.as_str()) {
                assert_eq!(
                    lookup.outcome,
                    LookupOutcome::NotEligible,
                    "{} in {} under {context_world:?}",
                    member.container,
                    name
                );
            } else {
                // The world's six archives all hold this name: the designed
                // order would pick its `texture.zbd` and shadow the five
                // `rtexture*.zbd` tiers, and that preference is only designed,
                // so *every* eligible lookup — the primary's included — is
                // refused instead of served. Nothing inside a world reaches
                // `Own` while the tiers disagree with the primary.
                match &lookup.outcome {
                    LookupOutcome::Blocked { selected, shadowed } => {
                        assert_eq!(
                            &selected.mount,
                            primary_of[archive.world_key.as_str()],
                            "{} in {name}: the world's {TEXTURE_ZBD} is the designed winner",
                            member.container
                        );
                        assert_eq!(
                            selected.container.as_str(),
                            world_primary(&retail, &archive.world_key)
                        );
                        assert_eq!(shadowed.len(), 5, "{} in {name}", member.container);
                        let shadowed_mounts: BTreeSet<&MountId> =
                            shadowed.iter().map(|origin| &origin.mount).collect();
                        assert_eq!(
                            shadowed_mounts,
                            tiers_of[archive.world_key.as_str()],
                            "{} in {name}",
                            member.container
                        );
                        for origin in shadowed {
                            assert_ne!(origin.sha256, selected.sha256);
                        }
                    }
                    other => panic!(
                        "{} in {name} under {context_world:?}: expected blocked, got {other}",
                        member.container
                    ),
                }
            }
        }
    }

    // The session (`zbd/c1`) resolves a shared name this world holds as
    // `UnmeasuredOrder` with its own `texture.zbd` selected, and a name only
    // another world holds as `NotFound` — never another world's bytes.
    let c1 = &world_names["zbd/c1"];
    assert_eq!(
        retail
            .session
            .context()
            .world_group
            .as_ref()
            .map(WorldGroup::logical_key),
        Some("zbd/c1".to_owned())
    );
    let shared = c1.iter().next().expect("c1 holds textures");
    match retail.session.resolve(&key(shared)) {
        Err(ResolveError::UnmeasuredOrder {
            selected, shadowed, ..
        }) => {
            assert_eq!(selected.mount, *primary_of["zbd/c1"]);
            assert_eq!(selected.container, world_primary(&retail, "zbd/c1"));
            assert_eq!(shadowed.len(), 5);
        }
        other => panic!("{shared} must stay blocked under zbd/c1: {other:?}"),
    }
    let elsewhere = holders
        .keys()
        .find(|name| !c1.contains(*name))
        .expect("another world holds names zbd/c1 does not");
    assert!(
        matches!(
            retail.session.resolve(&key(elsewhere)),
            Err(ResolveError::NotFound { .. })
        ),
        "zbd/c1 must not serve {elsewhere} from another world"
    );
}

fn world_primary<'a>(retail: &'a Retail, world: &str) -> &'a str {
    &retail
        .archives
        .iter()
        .find(|archive| archive.world_key == world && archive.primary)
        .expect("every world has a texture.zbd")
        .container
}

// ------------------------------------------------------ evidence harness ---

/// Evidence-report harness for task #346 (`docs/contracts/CLI-EVIDENCE.md`,
/// schema `schemas/evidence.schema.json`). Not an acceptance test: it fails
/// loudly when its inputs are missing. `CS_EVIDENCE_REVIEWER` names the agent
/// that ran it and is recorded in the report; it is not baked in, because the
/// reviewer regenerates the report on the rebased commit. Run from the
/// workspace root, after the acceptance suite, exactly as:
///
/// 1. ```sh
///    mkdir -p private/evidence/T346
///    cargo test --workspace --locked -- accept_f04_d_texture_member_collisions_ --include-ignored \
///      2>&1 | tee private/evidence/T346/cargo-test.log
///    ```
///    (record the exit status of `cargo test`, e.g. with `pipefail`.)
/// 2. ```sh
///    CS_EVIDENCE_DIR=private/evidence/T346 \
///    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
///    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f04_d_texture_member_collisions_ --include-ignored" \
///    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
///    CS_EVIDENCE_REVIEWER="<agent running this harness>" \
///      cargo test --locked -p cs_assets --test accept_f04_d_texture_member_collisions -- evidence_report_t346 --ignored
///    ```
/// 3. ```sh
///    python3 tools/validate_evidence.py private/evidence/T346/acceptance.json \
///      --artifact-root private/evidence/T346 --require-pass
///    ```
/// 4. Commit a copy of `acceptance.json` as `docs/findings/evidence/T346.json`.
///
/// Every field is derived from real inputs: the recorded log, production
/// discovery of `$CS_GAME_DIR`, the production collision comparison of every
/// mounted texture archive (`texture-member-collisions.json`: names, lengths
/// and hashes only), `rustc --version` and `Cargo.lock`.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_t346_writes_the_acceptance_report() {
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
        "CS_CANDIDATE_TREE must be the tree of the tested commit"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", log_path.display()));
    let suite = parse_suite(&log);
    assert!(
        suite.passed > 0 && suite.assertions.len() as u64 >= suite.passed,
        "no `{PREFIX}` results were understood in {}",
        log_path.display()
    );
    let retail_test = format!("{PREFIX}retail_each_world_keeps_its_textures");
    assert_eq!(
        suite
            .assertions
            .iter()
            .find(|(name, _)| *name == retail_test)
            .map(|(_, status)| *status),
        Some("pass"),
        "{retail_test} must have run and passed (step 1 needs --include-ignored and CS_GAME_DIR)"
    );
    assert!(
        suite
            .assertions
            .iter()
            .any(|(name, _)| !name.contains("_retail_")),
        "synthetic task tests must be present alongside the retail one"
    );

    let retail = retail();
    let collisions_path = evidence_dir.join("texture-member-collisions.json");
    fs::write(
        &collisions_path,
        texture_member_collisions_json(&candidate_tree, &retail),
    )
    .unwrap_or_else(|error| panic!("write {}: {error}", collisions_path.display()));
    let artifacts = [
        artifact(&log_path, "log"),
        artifact(&collisions_path, "json"),
    ];

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"T346\",\n\
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
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        jstr(&rustc_version()),
        jstr(&locked_version("bevy")),
        jstr(&locked_version("avian3d")),
        jstr(&iso_utc_now()),
        argv.iter()
            .map(|arg| jstr(arg))
            .collect::<Vec<_>>()
            .join(", "),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&retail.install_sha256),
        jstr(&retail.content_sha256),
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
                jstr(name)
            ))
            .collect::<Vec<_>>()
            .join(", "),
        artifacts
            .iter()
            .map(|(name, digest, kind)| format!(
                "{{\"path\": {}, \"sha256\": {digest:?}, \"kind\": {kind:?}}}",
                jstr(name)
            ))
            .collect::<Vec<_>>()
            .join(", "),
        jstr(&reviewer),
        jstr(
            "acceptance suite run locally with the retail capability; this harness derives every \
             field from the recorded log, production discovery of $CS_GAME_DIR, the production \
             collision comparison of every mounted retail texture archive, rustc and Cargo.lock; \
             which archive a world really uses stays unmeasured (recorded in \
             texture-member-collisions.json and docs/findings) and the claim is only implemented"
        ),
    );
    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// The measured texture-name collisions: every name two or more archives
/// hold (how many archives and worlds, how many distinct stored digests, the
/// per-context verdict) and the mounted archives. Names, lengths and hashes
/// only — never original bytes.
fn texture_member_collisions_json(candidate_tree: &str, retail: &Retail) -> String {
    let holders = holders(retail);
    let contexts: Vec<String> = retail
        .report
        .contexts
        .iter()
        .map(|context| {
            context
                .world_group
                .as_ref()
                .map_or_else(|| "null".to_owned(), |group| jstr(&group.logical_key()))
        })
        .collect();

    let mut world_names: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for archive in &retail.archives {
        world_names
            .entry(archive.world_key.clone())
            .or_default()
            .extend(archive.digests.keys().cloned());
    }

    let mut outcome_counts: BTreeMap<&str, u64> = BTreeMap::new();
    for comparison in &retail.report.comparisons {
        for lookup in &comparison.lookups {
            *outcome_counts.entry(lookup.outcome.label()).or_default() += 1;
        }
    }

    let mut names_json = Vec::new();
    for comparison in &retail.report.comparisons {
        let name = comparison.collision.file_name.as_str();
        let here = &holders[name];
        let worlds: BTreeSet<&str> = here
            .iter()
            .map(|container| archive_of(retail, container).world_key.as_str())
            .collect();
        let digests: BTreeSet<String> = here
            .iter()
            .map(|container| archive_of(retail, container).digests[name].to_hex())
            .collect();
        let mut verdicts = Vec::new();
        for (index, context) in retail.report.contexts.iter().enumerate() {
            let mut verdict = CollisionVerdict::DistinctByPath;
            for lookup in comparison
                .lookups
                .iter()
                .filter(|lookup| lookup.context == index)
            {
                let severity = match &lookup.outcome {
                    LookupOutcome::NotEligible | LookupOutcome::Own => {
                        CollisionVerdict::DistinctByPath
                    }
                    LookupOutcome::Other {
                        same_bytes: true, ..
                    } => CollisionVerdict::ShadowedByIdenticalBytes,
                    _ => CollisionVerdict::Conflicting,
                };
                verdict = verdict.max(severity);
            }
            let label = if context.world_group.is_some() {
                contexts[index].clone()
            } else {
                jstr("null")
            };
            verdicts.push(format!("{label}: {:?}", verdict.label()));
        }
        let selected = worlds
            .iter()
            .next()
            .map(|world| world_primary(retail, world))
            .expect("a collision is held by a world");
        names_json.push(format!(
            "{{\"name\": {}, \"worlds\": [{}], \"archive_count\": {}, \"distinct_digests\": {}, \
             \"digests_differ\": {}, \"designed_selected\": {}, \"verdicts\": {{{}}}}}",
            jstr(name),
            worlds
                .iter()
                .map(|world| jstr(world))
                .collect::<Vec<_>>()
                .join(", "),
            here.len(),
            digests.len(),
            digests.len() > 1,
            jstr(selected),
            verdicts.join(", "),
        ));
    }

    let archives_json: Vec<String> = retail
        .archives
        .iter()
        .map(|archive| {
            format!(
                "{{\"world\": {}, \"container\": {}, \"mount\": {}, \"precedence\": {:?}, \
                 \"members\": {}}}",
                jstr(&archive.world_key),
                jstr(&archive.container),
                jstr(archive.mount_id.as_str()),
                if archive.primary {
                    PrecedenceClass::MissionWorld
                } else {
                    PrecedenceClass::Shared
                }
                .label(),
                archive.member_count,
            )
        })
        .collect();
    let worlds_json: Vec<String> = world_names
        .iter()
        .map(|(world, names)| format!("{{\"world\": {}, \"names\": {}}}", jstr(world), names.len()))
        .collect();

    format!(
        "{{\n\
         \x20\"task_id\": \"T346\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"install_sha256\": {},\n\
         \x20\"content_sha256\": {},\n\
         \x20\"layout\": \"per-world texture.zbd (mission_world) + rtexture*.zbd (shared), all retail, bound to their world group (designed)\",\n\
         \x20\"precedence_status\": {},\n\
         \x20\"original_lookup_behavior\": \"unmeasured\",\n\
         \x20\"contexts\": [{}],\n\
         \x20\"archives\": [\n  {}\n ],\n\
         \x20\"world_names\": [{}],\n\
         \x20\"shared_name_count\": {},\n\
         \x20\"names_in_all_archives\": {},\n\
         \x20\"lookup_outcomes\": {{{}}},\n\
         \x20\"names\": [\n  {}\n ]\n\
         }}\n",
        jstr(candidate_tree),
        jstr(&iso_utc_now()),
        jstr(&retail.install_sha256),
        jstr(&retail.content_sha256),
        jstr(retail.report.precedence_status.label()),
        contexts.join(", "),
        archives_json.join(",\n  "),
        worlds_json.join(", "),
        holders.len(),
        holders.values().filter(|set| set.len() == 48).count(),
        outcome_counts
            .iter()
            .map(|(label, count)| format!("{}: {count}", jstr(label)))
            .collect::<Vec<_>>()
            .join(", "),
        names_json.join(",\n  "),
    )
}

// --------------------------------------------------------- harness utils ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!("{name} is not set: run the harness through the sequence in its doc comment")
    })
}

/// Cargo runs a test binary in the package root; re-anchor paths described
/// relative to the workspace root.
fn workspace_path(as_described: &str) -> PathBuf {
    let path = PathBuf::from(as_described);
    if path.is_absolute() {
        return path;
    }
    Path::new(&git(&["rev-parse", "--show-toplevel"])).join(path)
}

fn git(args: &[&str]) -> String {
    let output = Command::new("git").args(args).output().expect("git runs");
    assert!(output.status.success(), "git {args:?} failed");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn rustc_version() -> String {
    let output = Command::new("rustc")
        .arg("--version")
        .output()
        .expect("rustc runs");
    assert!(output.status.success(), "rustc --version failed");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// The locked version of one `Cargo.lock` package, read, never assumed.
fn locked_version(package: &str) -> String {
    let lock_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
    let lock = fs::read_to_string(&lock_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", lock_path.display()));
    let mut wanted = false;
    for line in lock.lines().map(str::trim) {
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

/// What the recorded `cargo test` output says happened.
#[derive(Default)]
struct Suite {
    discovered: u64,
    executed: u64,
    passed: u64,
    failed: u64,
    ignored: u64,
    /// `(test name, "pass" | "fail")`, in log order, deduplicated.
    assertions: Vec<(String, &'static str)>,
}

/// The libtest summaries and the per-test results of this task's tests.
fn parse_suite(log: &str) -> Suite {
    let mut suite = Suite::default();
    let mut pending: VecDeque<String> = VecDeque::new();
    for line in log.lines() {
        let trimmed = line.trim_start();
        if let Some(summary) = trimmed.strip_prefix("test result:") {
            for segment in summary.split(';') {
                let words: Vec<&str> = segment.split_whitespace().collect();
                for pair in words.windows(2) {
                    if let Ok(count) = pair[0].parse::<u64>() {
                        match pair[1] {
                            "passed" => suite.passed += count,
                            "failed" => suite.failed += count,
                            "ignored" => suite.ignored += count,
                            _ => continue,
                        }
                        break;
                    }
                }
            }
            continue;
        }
        if !pending.is_empty() && (trimmed == "ok" || trimmed == "FAILED") {
            let name = pending.pop_front().expect("pending test");
            record(
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
            cursor = &after[separator + 5..];
            if !name.starts_with(PREFIX) {
                continue;
            }
            match cursor.split_whitespace().next() {
                Some("ok") => record(&mut suite, name, "pass"),
                Some("FAILED") => record(&mut suite, name, "fail"),
                _ => pending.push_back(name),
            }
        }
    }
    suite.executed = suite.passed + suite.failed;
    suite.discovered = suite.executed + suite.ignored;
    suite
}

fn record(suite: &mut Suite, name: String, status: &'static str) {
    if !suite.assertions.iter().any(|(seen, _)| *seen == name) {
        suite.assertions.push((name, status));
    }
}

/// `(file name, sha256, kind)` of an artifact inside the evidence directory.
fn artifact(path: &Path, kind: &str) -> (String, String, String) {
    let bytes = fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    (
        path.file_name()
            .expect("artifact has a file name")
            .to_string_lossy()
            .into_owned(),
        sha256(&bytes).to_hex(),
        kind.to_owned(),
    )
}

/// A JSON string literal, quoted and escaped.
fn jstr(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            control if (control as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// RFC 3339 UTC with whole seconds (Hinnant's `civil_from_days`).
fn iso_utc_now() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after 1970")
        .as_secs() as i64;
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}
