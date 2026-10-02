//! Member-level collisions across every kind of mount in one content
//! session (task #342, the integration step of F04-D follow-up 2 in
//! `docs/findings/2026-09-28-f04-d-observed-collisions-and-async-cancel.md`).
//!
//! F04-D compared *file*-level collisions; #345 compared the members of
//! `crimson.rof` against `crimptch.rof`; #346 compared the members of the
//! per-world `texture.zbd` / `rtexture*.zbd` archives. Each of those ran in
//! its own session. Here the installation's file mounts
//! (`SessionBuilder::mount_installation`), the production ROF mounts
//! (`mount_rof_into`) and the texture members the production reader exposes
//! are mounted **together**, all retail, and
//! `ContentSession::collision_report` runs once under no world and under
//! every world group. Mounting them together can only add collisions across
//! kinds (a texture named like a ROF member, an archive file named like a
//! member); the test records whether any exist and keeps each kind's verdicts
//! as the sibling findings recorded them.
//!
//! Which archive the original engine prefers is **not measured**: a lookup
//! only the designed order decides between different bytes is refused with
//! `ResolveError::UnmeasuredOrder` (spec F04 non-negotiable behavior 2).
//!
//! The texture mounts are built in the test from the index
//! `cs_formats::texture::read_zbd_textures` exposes; there is still no
//! production texture archive mounter (see the #346 findings).
//!
//! Synthetic tests author their containers under the system temporary
//! directory (`common::TempTree`). The `retail_` test reads `$CS_GAME_DIR`
//! read-only and fails loudly without it; CI skips it.
//! `evidence_report_t342_writes_the_acceptance_report` is the evidence
//! harness (`docs/contracts/CLI-EVIDENCE.md`), not an acceptance test.

mod common;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use common::TempTree;
use cs_assets::install::{self, Discovery, content_fingerprint, fingerprint, sha256};
use cs_assets::rof::mount_rof_into;
use cs_assets::vfs::{
    CollisionReport, CollisionVerdict, ContentSession, INSTALL_NAMESPACE, LookupOutcome, Mount,
    MountBuilder, ResolveError, SessionBuilder,
};
use cs_formats::io::AllocationBudget;
use cs_formats::texture::read_zbd_textures;
use cs_formats::texture::zbd::{
    FLAG_BYTES_PER_PIXEL2, FLAG_NO_ALPHA, ZBD_TEXTURE_ENTRY_BYTES, ZBD_TEXTURE_HEADER_BYTES,
};
use cs_formats::{DIRECTORY_HEADER_BYTES, FLAG_COMPRESSED, FLAG_DIRECTORY, RECORD_BYTES};
use cs_types::asset_id::{
    AssetKey, MountId, MountNamespace, PrecedenceClass, ResolveContext, WorldGroup,
};
use cs_types::evidence::ClaimStatus;

/// The acceptance prefix of this task.
const PREFIX: &str = "accept_f04_d_member_collisions_";

/// The two retail ROF containers, spelled as the installation spells them.
const BASE_CONTAINER: &str = "GOSDATA/ASSETS/crimson.rof";
const PATCH_CONTAINER: &str = "GOSDATA/ASSETS/crimptch.rof";

/// The key space every texture archive is mounted in: an engine-authored
/// label, not an observed retail namespace.
const TEXTURE_NAMESPACE: &str = "texture";

/// The world's primary texture archive name.
const TEXTURE_ZBD: &str = "texture.zbd";

/// `NO_ALPHA | BYTES_PER_PIXEL2`: what a retail direct-color texture stores.
const OPAQUE: u32 = FLAG_BYTES_PER_PIXEL2 | FLAG_NO_ALPHA;

// ------------------------------------------------------------ fixtures ---

/// One authored file entry: its name, the bytes stored in the container and,
/// for a compressed entry, the decoded byte count.
struct Entry<'a> {
    name: &'a str,
    stored: Vec<u8>,
    decoded_len: Option<u32>,
}

impl<'a> Entry<'a> {
    fn plain(name: &'a str, bytes: &[u8]) -> Self {
        Self {
            name,
            stored: bytes.to_vec(),
            decoded_len: None,
        }
    }

    /// `payload` as a zlib stream of one *stored* DEFLATE block (RFC 1950
    /// header, RFC 1951 `BTYPE = 00`, Adler-32 trailer): a valid stream any
    /// inflater decodes, authored by hand so no compressor is needed, whose
    /// bytes differ from `payload` itself.
    fn zlib_stored(name: &'a str, payload: &[u8]) -> Self {
        let length = u16::try_from(payload.len()).expect("one stored block");
        let mut stream = vec![0x78, 0x01, 0x01];
        stream.extend_from_slice(&length.to_le_bytes());
        stream.extend_from_slice(&(!length).to_le_bytes());
        stream.extend_from_slice(payload);
        let (mut a, mut b) = (1u32, 0u32);
        for byte in payload {
            a = (a + u32::from(*byte)) % 65_521;
            b = (b + a) % 65_521;
        }
        stream.extend_from_slice(&((b << 16) | a).to_be_bytes());
        Self {
            name,
            stored: stream,
            decoded_len: Some(payload.len() as u32),
        }
    }
}

fn rof_names(names: &[&str]) -> Vec<u8> {
    let mut table = Vec::new();
    for name in names {
        table.extend_from_slice(name.as_bytes());
        table.push(0);
    }
    table
}

/// One directory block: header, 24-byte records (`start`, `raw_length` =
/// decoded count, `raw_length_on_disk` = stored count, `flags`,
/// `name_length`, `id`), name table.
fn block(records: &[[u32; 6]], names: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(records.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(names.len() as u32).to_le_bytes());
    for record in records {
        for word in record {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
    }
    bytes.extend_from_slice(names);
    bytes
}

/// `[root: <directory>/][<directory>: entries…][payloads…]` — the shape of
/// the retail overlap (`ASSETS/SCRIPTS/<name>` in both archives), one
/// directory deep.
fn rof_container(directory: &str, entries: &[Entry<'_>]) -> Vec<u8> {
    let root_names = rof_names(&[directory]);
    let root_len = DIRECTORY_HEADER_BYTES + RECORD_BYTES + root_names.len();
    let entry_names: Vec<&str> = entries.iter().map(|entry| entry.name).collect();
    let sub_names = rof_names(&entry_names);
    let sub_len = DIRECTORY_HEADER_BYTES + entries.len() * RECORD_BYTES + sub_names.len();

    let root = block(
        &[[
            root_len as u32,
            0,
            0,
            FLAG_DIRECTORY,
            directory.len() as u32 + 1,
            1,
        ]],
        &root_names,
    );
    let mut cursor = (root_len + sub_len) as u32;
    let mut records = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let stored = entry.stored.len() as u32;
        let (decoded, flags) = entry
            .decoded_len
            .map_or((stored, 0), |decoded| (decoded, FLAG_COMPRESSED));
        records.push([
            cursor,
            decoded,
            stored,
            flags,
            entry.name.len() as u32 + 1,
            10 + index as u32,
        ]);
        cursor += stored;
    }
    let mut bytes = root;
    bytes.extend_from_slice(&block(&records, &sub_names));
    for entry in entries {
        bytes.extend_from_slice(&entry.stored);
    }
    assert_eq!(bytes.len(), cursor as usize, "every payload placed once");
    bytes
}

/// The designed scope of the two archives: `crimson.rof` is the shared
/// base, `crimptch.rof` a patch overlay; both are original data (retail),
/// both serve every world, both answer the `install` key space the `rof`
/// inspection command uses.
fn rof_mount(container: &str, precedence: PrecedenceClass) -> MountBuilder {
    let id: String = format!("rof-{}", container.to_ascii_lowercase())
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();
    MountBuilder::new(
        MountId::new(&id).expect("a valid mount id"),
        MountNamespace::new(INSTALL_NAMESPACE).expect("a valid namespace"),
        precedence,
        container,
    )
    .retail()
}

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
        MountNamespace::new(TEXTURE_NAMESPACE).expect("a valid namespace"),
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

fn rof_key(spelling: &str) -> AssetKey {
    AssetKey::from_spelling(INSTALL_NAMESPACE, spelling, "default").expect("key is valid")
}

fn texture_key(spelling: &str) -> AssetKey {
    AssetKey::from_spelling(TEXTURE_NAMESPACE, spelling, "default").expect("key is valid")
}

fn world(spelling: &str) -> WorldGroup {
    WorldGroup::new(spelling).expect("the world is valid")
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

/// Which kind of mount a collision member comes from.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Kind {
    /// A file of the installation (`mount_installation`).
    File,
    /// A member of one of the two ROF archives.
    Rof,
    /// A member of a ZBD texture archive.
    Texture,
}

impl Kind {
    const fn label(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Rof => "rof_member",
            Self::Texture => "texture_member",
        }
    }
}

/// The kind of mount a member belongs to, from how the mounts were built:
/// ROF mounts are id-prefixed `rof-`, texture mounts live in their own key
/// space, everything else is an installation file mount.
fn kind_of(member: &cs_assets::vfs::CollisionMember) -> Kind {
    if member.namespace.as_str() == TEXTURE_NAMESPACE {
        Kind::Texture
    } else if member.mount.as_str().starts_with("rof-") {
        Kind::Rof
    } else {
        Kind::File
    }
}

fn comparison_kinds(comparison: &cs_assets::vfs::CollisionComparison) -> BTreeSet<Kind> {
    comparison.collision.members.iter().map(kind_of).collect()
}

/// Mounts the installation at `root`, both ROF archives and every texture
/// archive in `textures` (`(container, precedence, world, bytes)`) into one
/// session whose context selects `session_world`.
fn mount_everything(
    root: &Path,
    context: ResolveContext,
    diagnosis: &install::Diagnosis,
    textures: &[(String, PrecedenceClass, WorldGroup, Vec<u8>)],
) -> ContentSession {
    let mut builder = SessionBuilder::new(context);
    builder
        .mount_installation(root, diagnosis)
        .expect("the installation mounts");
    mount_rof_into(
        &mut builder,
        rof_mount(BASE_CONTAINER, PrecedenceClass::Shared),
        &root.join(BASE_CONTAINER),
    )
    .expect("the base archive mounts");
    mount_rof_into(
        &mut builder,
        rof_mount(PATCH_CONTAINER, PrecedenceClass::Patch),
        &root.join(PATCH_CONTAINER),
    )
    .expect("the patch archive mounts");
    for (container, precedence, world, bytes) in textures {
        builder
            .mount(archive_mount(container, *precedence, Some(world), bytes))
            .unwrap_or_else(|error| panic!("{container}: {error}"));
    }
    builder.open()
}

// ----------------------------------------------------------- synthetic ---

/// The synthetic installation: a world-one and a world-two `texture.zbd`
/// file (a file-level collision), `crimson.rof`/`crimptch.rof` sharing one
/// key with different bytes, and a ROF member named like a texture.
fn synthetic_session() -> (TempTree, ContentSession) {
    let tree = TempTree::new("t342-integrated");
    let c1 = package(&[("SHARED_TEX", 0xF800), ("NAMED_ALIKE.BM", 0x07E0)]);
    let c1_tier = package(&[("SHARED_TEX", 0x001F)]);
    let c2 = package(&[("SHARED_TEX", 0xFFE0)]);
    tree.write("ZBD/c1/texture.zbd", &c1);
    tree.write("ZBD/c2/texture.zbd", &c2);
    tree.write("ZBD/zrdr.zbd", b"shared zrdr");
    tree.write(
        BASE_CONTAINER,
        &rof_container(
            "SCRIPTS",
            &[
                Entry::plain("AIRFRAME.SCRIPT", b"base airframe script"),
                Entry::plain("NAMED_ALIKE.BM", b"a bitmap member"),
            ],
        ),
    );
    tree.write(
        PATCH_CONTAINER,
        &rof_container(
            "SCRIPTS",
            &[Entry::zlib_stored(
                "AIRFRAME.SCRIPT",
                b"patched airframe script",
            )],
        ),
    );
    let found = install::discover(tree.root()).expect("the installation is discovered");
    let context =
        ResolveContext::new(fingerprint(&found.manifest)).with_world_group(world("zbd/c1"));
    let textures = vec![
        (
            "ZBD/c1/texture.zbd".to_owned(),
            PrecedenceClass::MissionWorld,
            world("zbd/c1"),
            c1,
        ),
        (
            "ZBD/c1/rtexture2.zbd".to_owned(),
            PrecedenceClass::Shared,
            world("zbd/c1"),
            c1_tier,
        ),
        (
            "ZBD/c2/texture.zbd".to_owned(),
            PrecedenceClass::MissionWorld,
            world("zbd/c2"),
            c2,
        ),
    ];
    let session = mount_everything(tree.root(), context, &found.diagnosis, &textures);
    (tree, session)
}

fn synthetic_report(session: &ContentSession) -> CollisionReport {
    let installation = ResolveContext::new(session.context().installation);
    session.collision_report(&contexts(
        &installation,
        &[world("zbd/c1"), world("zbd/c2")],
    ))
}

/// All three kinds mount into one session without any mount or key
/// clashing, and each kind keeps its own verdicts: the file-level repeats
/// (`texture.zbd`) are `distinct_by_path`, the ROF key both archives hold and
/// the texture name a world's tier overrides are `conflicting`, and a texture
/// named like a ROF member never compares with it (other key space).
#[test]
fn accept_f04_d_member_collisions_each_kind_keeps_its_verdict_in_one_session() {
    let (_tree, session) = synthetic_session();
    assert!(session.rejected().is_empty(), "{:?}", session.rejected());
    let report = synthetic_report(&session);
    assert_eq!(report.precedence_status, ClaimStatus::Designed);
    assert_eq!(report.contexts.len(), 3);

    let mut by_kind: BTreeMap<Kind, Vec<&cs_assets::vfs::CollisionComparison>> = BTreeMap::new();
    for comparison in &report.comparisons {
        let kinds = comparison_kinds(comparison);
        if let [kind] = kinds.iter().copied().collect::<Vec<_>>()[..] {
            by_kind.entry(kind).or_default().push(comparison);
        } else {
            assert_eq!(
                comparison.verdict,
                CollisionVerdict::DistinctByPath,
                "{}: a collision across kinds is only a name repeat, never an overlap",
                comparison.collision.file_name
            );
        }
    }
    let verdicts = |kind| -> BTreeMap<String, &'static str> {
        by_kind
            .get(&kind)
            .map(|list| {
                list.iter()
                    .map(|c| {
                        (
                            c.collision.file_name.to_ascii_lowercase(),
                            c.verdict.label(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    assert_eq!(
        verdicts(Kind::File).get("texture.zbd"),
        Some(&"distinct_by_path")
    );
    assert_eq!(
        verdicts(Kind::Rof).get("airframe.script"),
        Some(&"conflicting")
    );
    assert_eq!(
        verdicts(Kind::Texture).get("shared_tex"),
        Some(&"conflicting")
    );
    // The only overlaps are the two the fixture authored.
    assert_eq!(report.conflicting().count(), 2);

    // The session refuses both overlaps instead of picking a copy.
    match session.resolve(&rof_key("SCRIPTS/AIRFRAME.SCRIPT")) {
        Err(ResolveError::UnmeasuredOrder {
            selected, shadowed, ..
        }) => {
            assert_eq!(selected.container, PATCH_CONTAINER);
            assert_eq!(shadowed.len(), 1);
            assert_eq!(shadowed[0].container, BASE_CONTAINER);
        }
        other => panic!("the patch must not win by the designed order alone: {other:?}"),
    }
    match session.resolve(&texture_key("SHARED_TEX")) {
        Err(ResolveError::UnmeasuredOrder {
            selected, shadowed, ..
        }) => {
            assert_eq!(selected.container, "ZBD/c1/texture.zbd");
            assert_eq!(shadowed.len(), 1);
            assert_eq!(shadowed[0].container, "ZBD/c1/rtexture2.zbd");
        }
        other => panic!("world one's texture.zbd must not win by design alone: {other:?}"),
    }
    // A texture only the primary holds is unaffected by the tier.
    session
        .resolve(&texture_key("NAMED_ALIKE.BM"))
        .expect("a name no tier overrides resolves");
    // The ROF member of the same file name is another key.
    session
        .resolve(&rof_key("SCRIPTS/NAMED_ALIKE.BM"))
        .expect("the ROF member resolves by its own key");

    // The designed order never serves an overlap: blocked where eligible,
    // never `ambiguous`. Only world two's own texture, which nothing there
    // shadows, is `own`.
    for comparison in report.conflicting() {
        for lookup in &comparison.lookups {
            let own_unshadowed = lookup.outcome == LookupOutcome::Own
                && comparison.collision.members[lookup.member].container == "ZBD/c2/texture.zbd";
            assert!(
                own_unshadowed
                    || matches!(
                        lookup.outcome,
                        LookupOutcome::Blocked { .. } | LookupOutcome::NotEligible
                    ),
                "{}: {:?}",
                comparison.collision.file_name,
                lookup
            );
        }
    }
}

/// The other world's same-named texture is never served by this world's
/// session: the c2 `SHARED_TEX` is not eligible under world one, and under
/// world two it is the only holder, so it resolves.
#[test]
fn accept_f04_d_member_collisions_worlds_stay_isolated_with_every_kind_mounted() {
    let (tree, _) = synthetic_session();
    let found = install::discover(tree.root()).expect("the installation is discovered");
    let installation = ResolveContext::new(fingerprint(&found.manifest));
    let textures = vec![
        (
            "ZBD/c1/texture.zbd".to_owned(),
            PrecedenceClass::MissionWorld,
            world("zbd/c1"),
            package(&[("SHARED_TEX", 0xF800)]),
        ),
        (
            "ZBD/c2/texture.zbd".to_owned(),
            PrecedenceClass::MissionWorld,
            world("zbd/c2"),
            package(&[("SHARED_TEX", 0xFFE0)]),
        ),
    ];
    let c1 = mount_everything(
        tree.root(),
        installation.clone().with_world_group(world("zbd/c1")),
        &found.diagnosis,
        &textures,
    );
    let c2 = mount_everything(
        tree.root(),
        installation.clone().with_world_group(world("zbd/c2")),
        &found.diagnosis,
        &textures,
    );
    let none = mount_everything(tree.root(), installation, &found.diagnosis, &textures);
    let in_c1 = c1.resolve(&texture_key("SHARED_TEX")).expect("c1 resolves");
    let in_c2 = c2.resolve(&texture_key("SHARED_TEX")).expect("c2 resolves");
    assert_eq!(in_c1.resolved().mount, mount_id("ZBD/c1/texture.zbd"));
    assert_eq!(in_c2.resolved().mount, mount_id("ZBD/c2/texture.zbd"));
    assert!(matches!(
        none.resolve(&texture_key("SHARED_TEX")),
        Err(ResolveError::NotFound { .. })
    ));
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

/// The retail installation with every kind of mount in one session, and its
/// collision report across no world and every world group.
struct Retail {
    install_sha256: String,
    content_sha256: String,
    session: ContentSession,
    report: CollisionReport,
    texture_archives: usize,
}

fn retail() -> Retail {
    let root = game_dir();
    let found = discovery();
    let worlds: Vec<WorldGroup> = found
        .diagnosis
        .world_groups
        .iter()
        .map(|group| WorldGroup::from_relative(group.clone()))
        .collect();
    assert_eq!(
        worlds.len(),
        8,
        "the reference installation has eight world groups"
    );
    let mut textures = Vec::new();
    for group in &found.diagnosis.world_groups {
        let spelling = group.as_str();
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
            if !primary && !(lower.starts_with("rtexture") && lower.ends_with(".zbd")) {
                continue;
            }
            textures.push((
                format!("{spelling}/{file_name}"),
                if primary {
                    PrecedenceClass::MissionWorld
                } else {
                    PrecedenceClass::Shared
                },
                WorldGroup::from_relative(group.clone()),
                fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display())),
            ));
        }
    }
    let installation = ResolveContext::new(fingerprint(&found.manifest));
    let session = mount_everything(
        &root,
        installation.clone().with_world_group(worlds[0].clone()),
        &found.diagnosis,
        &textures,
    );
    let report = session.collision_report(&contexts(&installation, &worlds));
    Retail {
        install_sha256: fingerprint(&found.manifest).to_hex(),
        content_sha256: content_fingerprint(&found.manifest).to_hex(),
        session,
        report,
        texture_archives: textures.len(),
    }
}

/// Collisions by kind: how many, and how many of each verdict.
fn tally(report: &CollisionReport) -> BTreeMap<Vec<Kind>, BTreeMap<&'static str, u64>> {
    let mut tally: BTreeMap<Vec<Kind>, BTreeMap<&'static str, u64>> = BTreeMap::new();
    for comparison in &report.comparisons {
        *tally
            .entry(comparison_kinds(comparison).into_iter().collect())
            .or_default()
            .entry(comparison.verdict.label())
            .or_default() += 1;
    }
    tally
}

/// Every member-level collision of the retail installation, with every kind
/// of mount in one session: the same results the sibling findings measured
/// separately (`crimptch.rof` 1 shared key, 63 name-only repeats; 1676
/// texture names all `conflicting`; 15 file names all `distinct_by_path`),
/// two loose files named like `crimson.rof` members (other paths, so other keys), and every shadowing blocked by
/// `UnmeasuredOrder`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f04_d_member_collisions_retail_every_member_collision_is_blocked_or_distinct() {
    let retail = retail();
    let Retail {
        session,
        report,
        texture_archives,
        ..
    } = &retail;
    let fingerprint = &retail.install_sha256;
    assert!(session.rejected().is_empty(), "{:?}", session.rejected());
    assert_eq!(report.precedence_status, ClaimStatus::Designed);
    assert_eq!(report.contexts.len(), 9, "no world plus eight world groups");
    assert_eq!(*texture_archives, 48, "8 world groups x 6 texture archives");

    let tally = tally(report);
    let counts = |kinds: &[Kind], verdict: &str| {
        tally
            .get(kinds)
            .and_then(|verdicts| verdicts.get(verdict))
            .copied()
            .unwrap_or(0)
    };
    // The reference installation (fingerprint named in every message).
    assert_eq!(
        tally.keys().cloned().collect::<Vec<_>>(),
        vec![
            vec![Kind::File],
            vec![Kind::File, Kind::Rof],
            vec![Kind::Rof],
            vec![Kind::Texture]
        ],
        "{fingerprint}: {tally:?}"
    );
    // The one collision that crosses kinds: two loose graphics files that
    // `crimson.rof` also stores under `ASSETS/GRAPHICS/` (another path, so
    // another key). Both stay reachable by their own key.
    assert_eq!(
        counts(&[Kind::File, Kind::Rof], "distinct_by_path"),
        2,
        "{fingerprint}"
    );
    let crossing: BTreeSet<String> = report
        .comparisons
        .iter()
        .filter(|comparison| comparison_kinds(comparison).len() > 1)
        .map(|comparison| comparison.collision.file_name.to_ascii_lowercase())
        .collect();
    assert_eq!(
        crossing,
        BTreeSet::from(["arial8.tga".to_owned(), "font.tga".to_owned()]),
        "{fingerprint}"
    );
    assert_eq!(
        counts(&[Kind::File], "distinct_by_path"),
        15,
        "{fingerprint}"
    );
    assert_eq!(
        counts(&[Kind::Rof], "distinct_by_path"),
        63,
        "{fingerprint}"
    );
    assert_eq!(counts(&[Kind::Rof], "conflicting"), 1, "{fingerprint}");
    assert_eq!(
        counts(&[Kind::Texture], "conflicting"),
        1676,
        "{fingerprint}"
    );
    assert_eq!(report.conflicting().count(), 1 + 1676, "{fingerprint}");
    assert_eq!(
        tally.values().flat_map(|v| v.values()).sum::<u64>() as usize,
        report.comparisons.len()
    );

    // Every overlap is blocked wherever it is eligible and never served.
    for comparison in report.conflicting() {
        let mut blocked = 0;
        for lookup in &comparison.lookups {
            match &lookup.outcome {
                LookupOutcome::Blocked { .. } => blocked += 1,
                LookupOutcome::NotEligible => {}
                other => panic!(
                    "{}: lookup of member {} under context {} is {other:?}, not blocked",
                    comparison.collision.file_name, lookup.member, lookup.context
                ),
            }
        }
        assert!(blocked > 0, "{}", comparison.collision.file_name);
    }
    // The file-level and name-only repeats reach no order: every member is
    // reachable by its own key and nothing is blocked or ambiguous.
    for comparison in report
        .comparisons
        .iter()
        .filter(|comparison| comparison.verdict == CollisionVerdict::DistinctByPath)
    {
        for member in 0..comparison.collision.members.len() {
            assert!(
                comparison
                    .lookups
                    .iter()
                    .any(|lookup| lookup.member == member && lookup.outcome == LookupOutcome::Own),
                "{}: member {member} resolves to itself under some context",
                comparison.collision.file_name
            );
        }
        assert!(comparison.lookups.iter().all(|lookup| matches!(
            lookup.outcome,
            LookupOutcome::Own | LookupOutcome::NotEligible
        )));
    }

    // The session's own `resolve` refuses what the report calls blocked.
    assert!(matches!(
        session.resolve(&rof_key("ASSETS/SCRIPTS/AIRFRAME.SCRIPT")),
        Err(ResolveError::UnmeasuredOrder { .. })
    ));
    let in_session_world = report
        .conflicting()
        .flat_map(|comparison| comparison.collision.members.iter())
        .find(|member| {
            kind_of(member) == Kind::Texture
                && member.container.to_ascii_lowercase().starts_with("zbd/c1/")
        })
        .expect("the session's world holds a texture that its archives overlap");
    assert!(
        matches!(
            session.resolve(&in_session_world.key()),
            Err(ResolveError::UnmeasuredOrder { .. })
        ),
        "{}",
        in_session_world.spelling
    );
}

// ------------------------------------------------------ evidence harness ---

/// Evidence-report harness for task #342 (`docs/contracts/CLI-EVIDENCE.md`,
/// schema `schemas/evidence.schema.json`). Not an acceptance test: it fails
/// loudly when its inputs are missing. `CS_EVIDENCE_REVIEWER` names the agent
/// that ran it and is recorded in the report; it is not baked in, because the
/// reviewer regenerates the report on the rebased commit. Run from the
/// workspace root, after the acceptance suite, exactly as:
///
/// 1. ```sh
///    mkdir -p private/evidence/T342
///    cargo test --workspace --locked -- accept_f04_d_member_collisions_ --include-ignored \
///      2>&1 | tee private/evidence/T342/cargo-test.log
///    ```
///    (record the exit status of `cargo test`, e.g. with `pipefail`.)
/// 2. ```sh
///    CS_EVIDENCE_DIR=private/evidence/T342 \
///    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
///    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f04_d_member_collisions_ --include-ignored" \
///    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
///    CS_EVIDENCE_REVIEWER="<agent running this harness>" \
///      cargo test --locked -p cs_assets --test accept_f04_d_member_collisions -- evidence_report_t342 --ignored
///    ```
/// 3. ```sh
///    python3 tools/validate_evidence.py private/evidence/T342/acceptance.json \
///      --artifact-root private/evidence/T342 --require-pass
///    ```
/// 4. Commit a copy of `acceptance.json` as `docs/findings/evidence/T342.json`.
///
/// Every field is derived from real inputs: the recorded log, production
/// discovery of `$CS_GAME_DIR`, the production collision comparison of the
/// two mounted archives and the production reads of every shared member
/// (`member-collisions.json`: names, lengths and hashes only),
/// `rustc --version` and `Cargo.lock`.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_t342_writes_the_acceptance_report() {
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
    // Who ran the harness. The report is regenerated by the reviewer on the
    // rebased commit, so the name cannot be baked into this file.
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
    let retail_test = format!("{PREFIX}retail_every_member_collision_is_blocked_or_distinct");
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
            .any(|(name, _)| !name.starts_with(&format!("{PREFIX}retail_"))),
        "synthetic task tests must be present alongside the retail one"
    );

    let retail = retail();
    let collisions_path = evidence_dir.join("member-collisions.json");
    fs::write(
        &collisions_path,
        integrated_collisions_json(&candidate_tree, &retail),
    )
    .unwrap_or_else(|error| panic!("write {}: {error}", collisions_path.display()));
    let artifacts = [
        artifact(&log_path, "log"),
        artifact(&collisions_path, "json"),
    ];

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"T342\",\n\
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
            "acceptance suite run locally with the retail capability; this harness derives \
             every field from the recorded log, production discovery of $CS_GAME_DIR, the \
             production collision comparison of installation, ROF and texture-archive mounts in one session, rustc and Cargo.lock; the original lookup order stays \
             unmeasured (recorded in member-collisions.json and docs/findings) and the \
             claim is only implemented"
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

/// The measured collisions of the integrated session: verdict counts per
/// kind, lookup outcome counts per kind and the session's own refusal of the
/// ROF overlap. Names, counts and hashes only — never original bytes.
fn integrated_collisions_json(candidate_tree: &str, retail: &Retail) -> String {
    let Retail {
        session, report, ..
    } = retail;
    let contexts: Vec<String> = report
        .contexts
        .iter()
        .map(|context| {
            context.world_group.as_ref().map_or_else(
                || "null".to_owned(),
                |group| jstr(group.as_relative().as_str()),
            )
        })
        .collect();
    let mut verdicts: BTreeMap<&str, BTreeMap<&str, u64>> = BTreeMap::new();
    let mut outcomes: BTreeMap<&str, BTreeMap<&str, u64>> = BTreeMap::new();
    let mut cross_kind = 0u64;
    let mut overlaps = Vec::new();
    for comparison in &report.comparisons {
        let kinds = comparison_kinds(comparison);
        let [kind] = kinds.iter().copied().collect::<Vec<_>>()[..] else {
            cross_kind += 1;
            continue;
        };
        *verdicts
            .entry(kind.label())
            .or_default()
            .entry(comparison.verdict.label())
            .or_default() += 1;
        for lookup in &comparison.lookups {
            *outcomes
                .entry(kind.label())
                .or_default()
                .entry(lookup.outcome.label())
                .or_default() += 1;
        }
        if kind == Kind::Rof && comparison.verdict == CollisionVerdict::Conflicting {
            let members: Vec<String> = comparison
                .collision
                .members
                .iter()
                .map(|member| {
                    format!(
                        "{{\"container\": {}, \"spelling\": {}, \"stored_len\": {}, \"stored_sha256\": {}}}",
                        jstr(&member.container),
                        jstr(&member.spelling),
                        member.size_bytes,
                        member.sha256.map_or_else(|| "null".to_owned(), |h| jstr(&h.to_hex())),
                    )
                })
                .collect();
            let resolve = match session.resolve(&comparison.collision.members[0].key()) {
                Ok(_) => "resolved",
                Err(ResolveError::UnmeasuredOrder { .. }) => "blocked_unmeasured_order",
                Err(ResolveError::Ambiguous { .. }) => "ambiguous",
                Err(ResolveError::NotFound { .. }) => "not_found",
            };
            overlaps.push(format!(
                "{{\"file_name\": {}, \"session_resolve\": {}, \"members\": [{}]}}",
                jstr(&comparison.collision.file_name),
                jstr(resolve),
                members.join(", ")
            ));
        }
    }
    let nested = |map: &BTreeMap<&str, BTreeMap<&str, u64>>| {
        map.iter()
            .map(|(kind, inner)| {
                format!(
                    "{}: {{{}}}",
                    jstr(kind),
                    inner
                        .iter()
                        .map(|(label, count)| format!("{}: {count}", jstr(label)))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "{{\n\
         \x20\"task_id\": \"T342\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"install_sha256\": {},\n\
         \x20\"layout\": \"installation file mounts + crimson.rof (shared) + crimptch.rof (patch) + 48 texture archives (texture.zbd mission_world, rtexture*.zbd shared, world-bound); all retail (designed)\",\n\
         \x20\"precedence_status\": {},\n\
         \x20\"original_lookup_behavior\": \"unmeasured\",\n\
         \x20\"contexts\": [{}],\n\
         \x20\"texture_archives\": {},\n\
         \x20\"cross_kind_collisions\": {cross_kind},\n\
         \x20\"verdicts_by_kind\": {{{}}},\n\
         \x20\"lookup_outcomes_by_kind\": {{{}}},\n\
         \x20\"rof_overlaps\": [\n  {}\n ]\n\
         }}\n",
        jstr(candidate_tree),
        jstr(&iso_utc_now()),
        jstr(&retail.install_sha256),
        jstr(report.precedence_status.label()),
        contexts.join(", "),
        retail.texture_archives,
        nested(&verdicts),
        nested(&outcomes),
        overlaps.join(",\n  "),
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
        // A status on its own line completes the earliest started test.
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
