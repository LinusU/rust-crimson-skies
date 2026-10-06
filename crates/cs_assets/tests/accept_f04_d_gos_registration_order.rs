//! The GOS registration order, pinned for GOS-style requests (task #686).
//!
//! F04-D measured the original engine's file lookup order by static analysis
//! (`docs/findings/2026-10-05-f04-d-original-lookup-order.md`, section D).
//! `roffile.dll`'s `AddNewROFDirectory` only `push_back`s, and
//! `MetaOpenFile` walks the registered sources in that order and takes the
//! first that has the name. So the GOS key space has a rule the rest of the
//! VFS does not: **registration order, first hit wins**, not a precedence
//! class. `cs_assets::vfs::gos` registers the sources in that order and
//! `Vfs::resolve` answers a `gos` key in it.
//!
//! What these tests pin:
//!
//! * the order itself — patch, `crimson.rof`, the loose UI asset directory,
//!   the current directory — and that it, not precedence, decides;
//! * the registry condition on step 1 as an **explicit documented option**:
//!   a stated `EXE Path` with the container present registers the patch, and
//!   the registry-key-absent case skips it and starts at `crimson.rof`;
//! * that the shadowed origins stay visible in the trace, so a GOS answer is
//!   never a silent first-wins map (spec F04 non-negotiable behavior 3);
//! * that the unmeasured name-matching rule (#693) is an input, so the
//!   `ARIAL8.TGA` case that depends on it is *not* silently decided here;
//! * and, on the original installation, that the measured member sets agree
//!   with what the order then implies.
//!
//! The status stays `inferred` (`GOS_ORDER_STATUS`): this is code-derived
//! evidence, never a measured retail run, and the registry case is not
//! measurable from files at all. No test claims otherwise.
//!
//! The synthetic tests author ROF containers under the system temporary
//! directory (`common::TempTree`) and mount them through the production
//! `cs_assets::vfs::gos::SessionBuilder::mount_gos_chain`. The `retail_`
//! test reads `$CS_GAME_DIR` read-only and fails loudly without it; CI skips
//! it. `evidence_report_t686_writes_the_acceptance_report` is the evidence
//! harness (`docs/contracts/CLI-EVIDENCE.md`), not an acceptance test.

mod common;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use common::TempTree;
use cs_assets::install::{self, content_fingerprint, fingerprint, sha256};
use cs_assets::vfs::{
    AttemptOutcome, CURRENT_DIRECTORY_MOUNT_ID, CollisionReport, ContentSession, ExePathOrigin,
    GOS_NAMESPACE, GOS_ORDER_STATUS, GosChain, GosInstall, GosNameMatch, GosSource, LOOSE_MOUNT_ID,
    LookupOrder, LookupOrderStatus, MAIN_MOUNT_ID, PATCH_MOUNT_ID, ResolveError, SessionBuilder,
    gos_key,
};
use cs_formats::{DIRECTORY_HEADER_BYTES, FLAG_COMPRESSED, FLAG_DIRECTORY, RECORD_BYTES};
use cs_types::asset_id::{AssetKey, PrecedenceClass, ResolveContext};
use cs_types::evidence::{ClaimStatus, ContentHash};

/// The acceptance prefix of this task.
const PREFIX: &str = "accept_f04_d_gos_registration_order_";

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

fn names(names: &[&str]) -> Vec<u8> {
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

/// A two-level container, `[root: <a>/<b>/][<a>][<b>: entries]`: the retail
/// shape, whose members read `ASSETS/GRAPHICS/ARIAL8.TGA`.
fn nested_container(first: &str, second: &str, entries: &[Entry<'_>]) -> Vec<u8> {
    let first_names = names(&[second]);
    let first_len = DIRECTORY_HEADER_BYTES + RECORD_BYTES + first_names.len();
    let entry_names: Vec<&str> = entries.iter().map(|entry| entry.name).collect();
    let second_names = names(&entry_names);
    let second_len = DIRECTORY_HEADER_BYTES + entries.len() * RECORD_BYTES + second_names.len();

    let root_names = names(&[first]);
    let root_len = DIRECTORY_HEADER_BYTES + RECORD_BYTES + root_names.len();
    let root = block(
        &[[
            // The first block begins right after the root block, so the root
            // record's `start` is the root block's own length.
            root_len as u32,
            0,
            0,
            FLAG_DIRECTORY,
            first.len() as u32 + 1,
            1,
        ]],
        &root_names,
    );
    let first_block = block(
        &[[
            // The second block begins right after this one.
            (root_len + first_len) as u32,
            0,
            0,
            FLAG_DIRECTORY,
            second.len() as u32 + 1,
            2,
        ]],
        &first_names,
    );
    let mut cursor = (root_len + first_len + second_len) as u32;
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
    bytes.extend_from_slice(&first_block);
    bytes.extend_from_slice(&block(&records, &second_names));
    for entry in entries {
        bytes.extend_from_slice(&entry.stored);
    }
    assert_eq!(bytes.len(), cursor as usize, "every payload placed once");
    bytes
}

/// The two containers and the loose tree a synthetic installation is built
/// from, spelled as the installation spells them.
struct Installation {
    tree: TempTree,
    /// `<EXE Path>` this fixture's "registry key" names.
    exe_path: PathBuf,
    /// A separate directory standing in for the process's current one.
    current_directory: PathBuf,
}

impl Installation {
    /// Writes `patch` as `GOSDATA/Assets/crimptch.rof`, `main` as
    /// `GOSDATA/Assets/crimson.rof` and `loose` as loose files whose spellings
    /// are relative to `GOSDATA` — which is what the third registered source
    /// is rooted at — in the case the original installation uses, so a loose
    /// member's own spelling is the same on a
    /// case-sensitive host and a case-insensitive one, and a case-only
    /// collision with a container member is genuinely a case-only one.
    fn new(label: &str, patch: &[u8], main: &[u8], loose: &[(&str, &[u8])]) -> Self {
        let tree = TempTree::new(label);
        tree.write("GOSDATA/Assets/crimptch.rof", patch);
        tree.write("GOSDATA/Assets/crimson.rof", main);
        for (spelling, bytes) in loose {
            tree.write(&format!("GOSDATA/{spelling}"), bytes);
        }
        let exe_path = tree.root().to_path_buf();
        let current_directory = exe_path.join("workdir");
        fs::create_dir_all(&current_directory).expect("the workdir fixture is created");
        fs::write(
            current_directory.join("workdir.script"),
            b"the current directory holds this",
        )
        .expect("the current-directory fixture file is written");
        Self {
            tree,
            exe_path,
            current_directory,
        }
    }

    /// Adds files to the current directory, which is step 4 of the order,
    /// spelled relative to that directory.
    fn with_current_files(self, files: &[(&str, &[u8])]) -> Self {
        for (spelling, bytes) in files {
            let path = self.current_directory.join(spelling);
            fs::create_dir_all(path.parent().expect("a parent")).expect("dirs are created");
            fs::write(&path, bytes).expect("current-directory bytes are written");
        }
        self
    }

    fn host_root(&self) -> &Path {
        self.tree.root()
    }

    /// The chain request for a registry key that names this installation,
    /// which is how a registered installation is set up.
    fn with_registry(&self) -> GosInstall<'_> {
        GosInstall::new(
            self.host_root(),
            ExePathOrigin::inspect_registry(&self.exe_path),
        )
        .with_current_directory(&self.current_directory)
    }
}

/// One mounted chain plus the sources its containers can be read through.
struct Chain {
    session: ContentSession,
    chain: GosChain,
}

fn mount(install: &GosInstall<'_>) -> Chain {
    mount_with(&install_context(install), install)
}

fn mount_with(context: &ResolveContext, install: &GosInstall<'_>) -> Chain {
    let mut builder = SessionBuilder::new(context.clone());
    let chain = builder.mount_gos_chain(install).expect("the chain mounts");
    Chain {
        session: builder.open(),
        chain,
    }
}

/// A context whose fingerprint is the install root, so a synthetic fixture
/// needs no discovered manifest.
fn install_context(install: &GosInstall<'_>) -> ResolveContext {
    ResolveContext::new(sha256(install.host_root.as_os_str().as_encoded_bytes()))
}

/// A context that names no particular installation, for a chain whose
/// members are only checked for identity and not read.
fn bare_context() -> ResolveContext {
    ResolveContext::new(ContentHash::from_hex(&"0".repeat(64)).expect("64 hex zeros"))
}

/// The attempts of a trace as `(mount, outcome)` in the order the trace
/// reports them.
fn attempts(asset: &cs_assets::vfs::SessionAsset) -> Vec<(String, AttemptOutcome)> {
    asset
        .resolved()
        .trace
        .attempts
        .iter()
        .map(|attempt| (attempt.mount.to_string(), attempt.outcome.clone()))
        .collect()
}

fn mount_ids(chain: &Chain) -> Vec<String> {
    chain
        .chain
        .steps()
        .iter()
        .map(|step| step.mount.to_string())
        .collect()
}

// ----------------------------------------------------------- synthetic ---

/// The order the original registers, with the patch present: the patch
/// container, then `crimson.rof`, the loose UI asset directory and the
/// current directory — in that order, every step mounted and no step
/// skipped.
#[test]
fn accept_f04_d_gos_registration_order_registers_four_sources_in_order() {
    let install = Installation::new(
        "t686-order",
        &nested_container(
            "ASSETS",
            "SCRIPTS",
            &[Entry::plain("AIRFRAME.SCRIPT", b"patched")],
        ),
        &nested_container(
            "ASSETS",
            "SCRIPTS",
            &[Entry::plain("AIRFRAME.SCRIPT", b"main")],
        ),
        &[("assets/scripts/main_only.script", b"loose")],
    );
    let chain = mount(&install.with_registry());

    assert_eq!(
        chain.chain.order(),
        [
            "patch_container",
            "main_container",
            "loose_ui_assets",
            "current_directory"
        ]
    );
    assert_eq!(
        mount_ids(&chain),
        [
            PATCH_MOUNT_ID,
            MAIN_MOUNT_ID,
            LOOSE_MOUNT_ID,
            CURRENT_DIRECTORY_MOUNT_ID
        ]
    );
    assert!(
        chain.chain.unregistered().is_empty(),
        "every step registered"
    );
    assert!(chain.chain.patch().is_registered());
    assert_eq!(
        chain.chain.patch().label(),
        "registered",
        "a stated EXE Path with the container present registers the patch"
    );
    assert_eq!(chain.chain.order_status(), ClaimStatus::Inferred);
    assert_ne!(
        chain.chain.order_status(),
        ClaimStatus::VerifiedOriginal,
        "code-derived evidence is never presented as a measured original run"
    );

    // The chain is a record of what was registered, not just of the order:
    // each step says which mount holds it, from which container and with how
    // many members.
    for step in chain.chain.steps() {
        assert!(step.members > 0, "{step:?} contributes members");
        assert!(
            step.container.exists(),
            "{step:?} names something that was read"
        );
        let mount = chain
            .session
            .mounts()
            .find(|m| m.id() == &step.mount)
            .unwrap_or_else(|| panic!("{} is mounted", step.mount));
        assert_eq!(mount.namespace().as_str(), GOS_NAMESPACE);
        assert_eq!(mount.member_count(), step.members);
        assert!(mount.is_retail(), "GOS sources are original data");
    }
}

/// The order decides, not the precedence class: with the patch present the
/// patch's member wins over the main container's member of the same key even
/// though the two hold different bytes — and unlike an `install`-namespace
/// lookup of the same pair, this is **not** refused as
/// `UnmeasuredOrder`, because the deciding order is the original's
/// registration order rather than this workspace's design.
#[test]
fn accept_f04_d_gos_registration_order_patch_wins_and_is_not_blocked() {
    let install = Installation::new(
        "t686-patch-wins",
        &nested_container(
            "ASSETS",
            "SCRIPTS",
            &[Entry::zlib_stored("AIRFRAME.SCRIPT", b"patched airframe")],
        ),
        &nested_container(
            "ASSETS",
            "SCRIPTS",
            &[Entry::zlib_stored("AIRFRAME.SCRIPT", b"main airframe")],
        ),
        &[],
    );
    let chain = mount(&install.with_registry());
    let key = gos_key("ASSETS/SCRIPTS/AIRFRAME.SCRIPT").expect("a valid key");

    let asset = chain
        .session
        .resolve(&key)
        .expect("the GOS order decides this lookup, so it resolves");
    assert_eq!(asset.resolved().mount.as_str(), PATCH_MOUNT_ID);
    assert_eq!(asset.resolved().precedence, PrecedenceClass::Patch);
    assert_eq!(
        asset.resolved().span.container_path(),
        "GOSDATA/Assets/crimptch.rof"
    );
    assert_eq!(
        asset.resolved().span.member_key(),
        Some("ASSETS/SCRIPTS/AIRFRAME.SCRIPT")
    );

    // The trace states what decided and how well that is known, and keeps
    // the shadowed origin visible instead of reporting a silent first win.
    assert_eq!(
        asset.resolved().trace.order,
        LookupOrderStatus {
            order: LookupOrder::GosRegistration,
            status: GOS_ORDER_STATUS,
        }
    );
    assert_eq!(
        attempts(&asset),
        [
            (PATCH_MOUNT_ID.to_owned(), AttemptOutcome::Selected),
            (MAIN_MOUNT_ID.to_owned(), AttemptOutcome::Candidate),
            (LOOSE_MOUNT_ID.to_owned(), AttemptOutcome::Miss),
            (CURRENT_DIRECTORY_MOUNT_ID.to_owned(), AttemptOutcome::Miss),
        ],
        "the later registration is a candidate that lost, not a tie"
    );

    // The bytes are the patch's, read through the production ROF reader the
    // chain keeps for its containers.
    let bytes = chain
        .chain
        .read(&chain.session, &asset)
        .expect("the served member reads");
    assert_eq!(
        bytes, b"patched airframe",
        "the origin the order chose is the origin whose bytes are read"
    );
}

/// The registry condition on step 1 is an explicit option, and both cases
/// work: with the key naming a path whose `crimptch.rof` exists the chain
/// starts at the patch, and with no such key the patch is never registered
/// and the same request is served by `crimson.rof` instead. The difference is
/// *registration*, not a lost race, so the trace shows one fewer source.
#[test]
fn accept_f04_d_gos_registration_order_registry_absent_skips_the_patch() {
    let patch_bytes = nested_container(
        "ASSETS",
        "SCRIPTS",
        &[Entry::plain("AIRFRAME.SCRIPT", b"patched airframe")],
    );
    let install = Installation::new(
        "t686-registry",
        &patch_bytes,
        &nested_container(
            "ASSETS",
            "SCRIPTS",
            &[Entry::plain("AIRFRAME.SCRIPT", b"main airframe")],
        ),
        &[],
    );
    let key = gos_key("ASSETS/SCRIPTS/AIRFRAME.SCRIPT").expect("a valid key");

    // Case A: the registry names this installation, whose patch exists.
    let with_registry = mount(&install.with_registry());
    assert!(with_registry.chain.patch().is_registered());
    assert_eq!(
        with_registry
            .session
            .resolve(&key)
            .expect("the patch serves the request")
            .resolved()
            .mount
            .as_str(),
        PATCH_MOUNT_ID
    );

    // Case B: no such registry key. The patch is never registered, so the
    // order starts at `crimson.rof` — the documented consequence, not a
    // fallback invented here.
    let absent = GosInstall::new(install.host_root(), ExePathOrigin::RegistryKeyAbsent)
        .with_current_directory(&install.current_directory);
    let absent_chain = mount(&absent);
    assert_eq!(absent_chain.chain.patch().label(), "registry_key_absent");
    assert!(
        !absent_chain.chain.patch().is_registered(),
        "no registry key means the patch was never registered"
    );
    assert_eq!(
        absent_chain.chain.order(),
        ["main_container", "loose_ui_assets", "current_directory"]
    );
    assert_eq!(
        absent_chain.chain.unregistered(),
        [GosSource::PatchContainer],
        "the report names what is missing, which is the registry case"
    );
    assert!(
        absent_chain
            .chain
            .unregistered()
            .contains(&GosSource::PatchContainer)
            && !absent_chain
                .chain
                .unregistered()
                .contains(&GosSource::MainContainer)
    );

    let asset = absent_chain
        .session
        .resolve(&key)
        .expect("the main container serves the request instead");
    assert_eq!(asset.resolved().mount.as_str(), MAIN_MOUNT_ID);
    assert_eq!(
        attempts(&asset),
        [
            (MAIN_MOUNT_ID.to_owned(), AttemptOutcome::Selected),
            (LOOSE_MOUNT_ID.to_owned(), AttemptOutcome::Miss),
            (CURRENT_DIRECTORY_MOUNT_ID.to_owned(), AttemptOutcome::Miss),
        ],
        "the patch is not an attempt at all: it was never registered"
    );
}

/// The third registry case: the key exists and names a path, but no patch
/// container is there, so the chain starts at `crimson.rof` for the
/// original's own reason. It is reported as its own case rather than folded
/// into the key-absent one, because the two are different facts about the
/// machine.
#[test]
fn accept_f04_d_gos_registration_order_registry_without_patch_is_its_own_case() {
    let install = Installation::new(
        "t686-no-patch",
        &nested_container(
            "ASSETS",
            "SCRIPTS",
            &[Entry::plain("AIRFRAME.SCRIPT", b"patched")],
        ),
        &nested_container(
            "ASSETS",
            "SCRIPTS",
            &[Entry::plain("AIRFRAME.SCRIPT", b"main")],
        ),
        &[],
    );
    // A registry key naming a path that holds no patch container at all.
    let elsewhere = install.host_root().join("elsewhere");
    fs::create_dir_all(&elsewhere).expect("the fixture directory exists");
    let absent = GosInstall::new(
        install.host_root(),
        ExePathOrigin::inspect_registry(&elsewhere),
    )
    .with_current_directory(&install.current_directory);

    assert_eq!(
        absent.exe_path,
        ExePathOrigin::RegistryWithoutPatch(elsewhere.clone()),
        "the key named a path, and the container is not there"
    );
    let chain = mount(&absent);
    assert_eq!(chain.chain.patch().label(), "container_absent");
    assert_eq!(
        chain.chain.patch().to_string(),
        format!(
            "skipped: no crimptch.rof under {}",
            ExePathOrigin::patch_container(&elsewhere).display()
        )
    );
    assert_eq!(
        chain.chain.order(),
        ["main_container", "loose_ui_assets", "current_directory"]
    );
    assert_eq!(
        chain
            .session
            .resolve(&gos_key("ASSETS/SCRIPTS/AIRFRAME.SCRIPT").expect("a key"))
            .expect("the main container serves it")
            .resolved()
            .mount
            .as_str(),
        MAIN_MOUNT_ID
    );
}

/// Steps 3 and 4 keep their relative order: a name the containers do not
/// hold is served by the loose UI asset directory, and only a name *that*
/// directory does not hold falls through to the current directory. Swapping
/// the two would change both answers, so both are pinned.
#[test]
fn accept_f04_d_gos_registration_order_loose_directory_precedes_the_current_one() {
    let install = Installation::new(
        "t686-loose",
        &nested_container(
            "ASSETS",
            "SCRIPTS",
            &[Entry::plain("PATCHED.SCRIPT", b"in the patch")],
        ),
        &nested_container(
            "ASSETS",
            "SCRIPTS",
            &[
                Entry::plain("MAIN.SCRIPT", b"in the main container"),
                Entry::plain("BOTH.SCRIPT", b"main copy of the shared script"),
            ],
        ),
        &[
            ("Assets/GRAPHICS/arial8.tga", b"loose arial8"),
            (
                "Assets/SCRIPTS/both.script",
                b"loose copy of the shared script",
            ),
            ("Assets/SCRIPTS/shared.script", b"loose shared"),
        ],
    )
    .with_current_files(&[
        ("ASSETS/CURRENT_ONLY.SCRIPT", b"in the current directory"),
        ("ASSETS/SCRIPTS/SHARED.SCRIPT", b"current shared"),
    ]);
    let chain = mount(&install.with_registry());

    let resolve = |spelling: &str| {
        chain
            .session
            .resolve(&gos_key(spelling).expect("a valid key"))
            .unwrap_or_else(|error| panic!("{spelling} resolves: {error}"))
    };

    // Step 2 wins over step 3 for the name both hold, even though the loose
    // copy spells it in the case the request used.
    assert_eq!(
        resolve("ASSETS/SCRIPTS/BOTH.SCRIPT")
            .resolved()
            .mount
            .as_str(),
        MAIN_MOUNT_ID
    );
    // Step 3 wins over step 4 for a name only they both hold.
    assert_eq!(
        resolve("ASSETS/SCRIPTS/SHARED.SCRIPT")
            .resolved()
            .mount
            .as_str(),
        LOOSE_MOUNT_ID
    );
    // Step 3 serves a name no container holds, keeping its own spelling.
    let loose = resolve("ASSETS/GRAPHICS/ARIAL8.TGA");
    assert_eq!(loose.resolved().mount.as_str(), LOOSE_MOUNT_ID);
    assert_eq!(
        loose.resolved().span.member_key(),
        Some("Assets/GRAPHICS/arial8.tga"),
        "the loose file keeps its own spelling"
    );
    // Step 4 serves a name nothing earlier holds.
    let current = resolve("ASSETS/CURRENT_ONLY.SCRIPT");
    assert_eq!(
        current.resolved().mount.as_str(),
        CURRENT_DIRECTORY_MOUNT_ID
    );
    assert_eq!(
        current.resolved().span.container_path(),
        ".",
        "the current directory is its own container"
    );
    // And nothing is invented for a name no source holds.
    let missing = chain
        .session
        .resolve(&gos_key("ASSETS/NOWHERE.SCRIPT").expect("a valid key"));
    assert!(
        matches!(missing, Err(ResolveError::NotFound { .. })),
        "{missing:?}"
    );
}

/// Leaving step 4 unregistered is allowed and recorded: a caller that does
/// not know the process's current directory says so rather than having one
/// guessed, and a name only that directory would hold is then simply not
/// found — with the report naming the step as unregistered.
#[test]
fn accept_f04_d_gos_registration_order_current_directory_may_be_left_unregistered() {
    let install = Installation::new(
        "t686-no-cwd",
        &nested_container("ASSETS", "SCRIPTS", &[Entry::plain("A.SCRIPT", b"a")]),
        &nested_container("ASSETS", "SCRIPTS", &[Entry::plain("B.SCRIPT", b"b")]),
        &[("Assets/SCRIPTS/loose.script", b"loose")],
    )
    .with_current_files(&[("assets/current.script", b"current")]);
    let request = GosInstall::new(
        install.host_root(),
        ExePathOrigin::inspect_registry(&install.exe_path),
    );
    assert!(
        request.current_directory.is_none(),
        "the default leaves step 4 unregistered"
    );

    let chain = mount(&request);
    assert_eq!(
        chain.chain.order(),
        ["patch_container", "main_container", "loose_ui_assets"]
    );
    assert_eq!(chain.chain.unregistered(), [GosSource::CurrentDirectory]);
    assert!(
        matches!(
            chain
                .session
                .resolve(&gos_key("ASSETS/CURRENT.SCRIPT").expect("a key")),
            Err(ResolveError::NotFound { .. })
        ),
        "an unregistered source cannot serve anything"
    );
    // The sources that *are* registered still answer.
    assert!(
        chain
            .session
            .resolve(&gos_key("ASSETS/SCRIPTS/LOOSE.SCRIPT").expect("a key"))
            .is_ok()
    );
}

/// The unmeasured name-matching rule (#693) is an input, and it changes the
/// answer for the case that depends on it: `crimson.rof` stores
/// `ASSETS/GRAPHICS/ARIAL8.TGA` while the loose tree holds
/// `assets/graphics/arial8.tga`, so which of the two a request gets is not
/// decided here. Both rules are implemented, both are stated by the caller,
/// and the one this VFS defaults to is visible.
#[test]
fn accept_f04_d_gos_registration_order_name_matching_is_an_explicit_input() {
    let install = Installation::new(
        "t686-case",
        &nested_container(
            "ASSETS",
            "SCRIPTS",
            &[Entry::plain("AIRFRAME.SCRIPT", b"patched")],
        ),
        &nested_container(
            "ASSETS",
            "GRAPHICS",
            &[Entry::plain("ARIAL8.TGA", b"container arial8")],
        ),
        &[("Assets/GRAPHICS/arial8.tga", b"loose arial8")],
    );

    // The default folds case and separators, so the container's uppercase
    // spelling answers the lowercase request — the ROF member, not the loose
    // file, because the container is registered first.
    let folding = mount(&install.with_registry());
    assert_eq!(folding.chain.name_match(), GosNameMatch::AsciiInsensitive);
    assert!(folding.chain.name_match().folds());
    let served = folding
        .session
        .resolve(&gos_key("assets/graphics/arial8.tga").expect("a valid key"))
        .expect("the request resolves under the folding rule");
    assert_eq!(served.resolved().mount.as_str(), MAIN_MOUNT_ID);
    assert_eq!(
        served.resolved().span.member_key(),
        Some("ASSETS/GRAPHICS/ARIAL8.TGA"),
        "the container's own spelling is kept"
    );

    // The exact rule answers only a byte-equal spelling, so the container's
    // uppercase name no longer answers the lowercase request, and the request
    // is served by the loose file — whose own spelling is
    // `Assets/GRAPHICS/arial8.tga`, the one spelling that matches exactly.
    let exact = mount(
        &install
            .with_registry()
            .with_name_match(GosNameMatch::ExactSpelling),
    );
    assert_eq!(exact.chain.name_match(), GosNameMatch::ExactSpelling);
    assert!(!exact.chain.name_match().folds());
    assert!(
        matches!(
            exact
                .session
                .resolve(&gos_key("assets/graphics/arial8.tga").expect("a valid key")),
            Err(ResolveError::NotFound { .. })
        ),
        "under the exact rule no source spells the name the way it was asked"
    );
    let served = exact
        .session
        .resolve(&gos_key("Assets/GRAPHICS/arial8.tga").expect("a valid key"))
        .expect("the loose spelling resolves under the exact rule");
    assert_eq!(served.resolved().mount.as_str(), LOOSE_MOUNT_ID);
    assert_eq!(
        served.resolved().span.member_key(),
        Some("Assets/GRAPHICS/arial8.tga")
    );
    // The container's uppercase spelling still answers its own request.
    assert_eq!(
        exact
            .session
            .resolve(&gos_key("ASSETS/GRAPHICS/ARIAL8.TGA").expect("a valid key"))
            .expect("the exact spelling resolves")
            .resolved()
            .mount
            .as_str(),
        MAIN_MOUNT_ID
    );

    // The part of the order that does *not* depend on the rule: the patch
    // shadows the main container for `ASSETS/SCRIPTS/AIRFRAME.SCRIPT` under
    // either rule, because both containers hold it under the same spelling.
    for matching in [GosNameMatch::AsciiInsensitive, GosNameMatch::ExactSpelling] {
        let chain = mount(&install.with_registry().with_name_match(matching));
        assert_eq!(
            chain
                .session
                .resolve(&gos_key("ASSETS/SCRIPTS/AIRFRAME.SCRIPT").expect("a key"))
                .expect("both containers hold this name")
                .resolved()
                .mount
                .as_str(),
            PATCH_MOUNT_ID,
            "the patch-over-main order is unconditional ({matching})"
        );
    }

    // One VFS answers one key space under one rule: a second, different rule
    // is refused rather than silently taking effect.
    let mut builder = SessionBuilder::new(install_context(&install.with_registry()));
    builder
        .mount_gos_chain(&install.with_registry())
        .expect("the first chain mounts");
    let conflict = builder.mount_gos_chain(
        &install
            .with_registry()
            .with_name_match(GosNameMatch::ExactSpelling),
    );
    assert!(
        conflict.is_err(),
        "two chains with different rules in one VFS are refused"
    );
}

/// A GOS request cannot address another key space, and the installed
/// namespaces keep their own rule: an `install` key is still decided by
/// precedence, so the same patch/main pair mounted there is blocked as
/// unmeasured rather than resolved by registration. The GOS namespace is an
/// addition, not a change of the existing contract.
#[test]
fn accept_f04_d_gos_registration_order_leaves_other_namespaces_on_precedence() {
    let install = Installation::new(
        "t686-other",
        &nested_container(
            "ASSETS",
            "SCRIPTS",
            &[Entry::plain("AIRFRAME.SCRIPT", b"patched")],
        ),
        &nested_container(
            "ASSETS",
            "SCRIPTS",
            &[Entry::plain("AIRFRAME.SCRIPT", b"main")],
        ),
        &[],
    );
    let chain = mount(&install.with_registry());

    // A GOS key addresses only the GOS namespace.
    let foreign = chain.session.resolve(
        &AssetKey::from_spelling("install", "ASSETS/SCRIPTS/AIRFRAME.SCRIPT", "default")
            .expect("a valid key"),
    );
    assert!(
        matches!(foreign, Err(ResolveError::NotFound { .. })),
        "{foreign:?}"
    );

    // And the trace of a GOS key reports the GOS order even when the
    // precedence classes would have ranked it differently.
    let asset = chain
        .session
        .resolve(&gos_key("ASSETS/SCRIPTS/AIRFRAME.SCRIPT").expect("a key"))
        .expect("the GOS order decides");
    assert_eq!(
        asset.resolved().trace.order.order,
        LookupOrder::GosRegistration
    );
    assert_ne!(
        asset.resolved().trace.order.order,
        LookupOrder::Precedence,
        "a GOS key is not decided by precedence"
    );
}

/// The ROF member collision report stays consistent with the order it now
/// implements: the patch-over-main pair is a collision whose verdict follows
/// the GOS lookup (which serves the patch) rather than the old
/// blocked-unmeasured-order classification, and every lookup in the report
/// agrees with what a session resolve returns.
#[test]
fn accept_f04_d_gos_registration_order_member_collisions_follow_the_gos_order() {
    let install = Installation::new(
        "t686-collisions",
        &nested_container(
            "ASSETS",
            "SCRIPTS",
            &[Entry::plain("AIRFRAME.SCRIPT", b"patched airframe")],
        ),
        &nested_container(
            "ASSETS",
            "SCRIPTS",
            &[
                Entry::plain("AIRFRAME.SCRIPT", b"main airframe"),
                Entry::plain("WEAPONS.SCRIPT", b"main weapons"),
            ],
        ),
        &[("arial8.tga", b"loose arial8")],
    );
    let chain = mount(&install.with_registry());

    let report: CollisionReport = chain
        .session
        .collision_report(&[chain.session.context().clone()]);
    assert_eq!(
        report.precedence_status,
        ClaimStatus::Designed,
        "the report still carries the precedence status for the namespaces it compares"
    );

    let airframe = report
        .comparisons
        .iter()
        .find(|comparison| comparison.collision.file_name == "airframe.script")
        .expect("airframe.script is a collision");
    assert_eq!(airframe.collision.members.len(), 2);
    assert_ne!(
        airframe.collision.distinct_digests(),
        1,
        "the two containers hold different bytes"
    );
    // The GOS lookup serves the patch, so the main container's member is
    // reported as `other_different_bytes` rather than as blocked: the order
    // that decided it is known.
    assert!(
        airframe
            .lookups
            .iter()
            .any(|lookup| lookup.outcome.label() == "other_different_bytes"),
        "{:?}",
        airframe.lookups
    );
    assert!(
        airframe
            .lookups
            .iter()
            .all(|lookup| lookup.outcome.label() != "blocked_unmeasured_order"),
        "a GOS collision is not blocked: {:?}",
        airframe.lookups
    );

    // Every member that only the main container holds still resolves to
    // itself, so the order changed nothing else.
    let weapons = chain
        .session
        .resolve(&gos_key("ASSETS/SCRIPTS/WEAPONS.SCRIPT").expect("a key"))
        .expect("a member only the main container holds resolves");
    assert_eq!(weapons.resolved().mount.as_str(), MAIN_MOUNT_ID);
}

/// A missing `crimson.rof` fails the chain at step 2 with that step named,
/// and nothing is reordered to work around it: the patch that mounted
/// before it stays, and no answer is produced.
#[test]
fn accept_f04_d_gos_registration_order_missing_main_container_names_its_step() {
    let install = Installation::new(
        "t686-no-main",
        &nested_container(
            "ASSETS",
            "SCRIPTS",
            &[Entry::plain("AIRFRAME.SCRIPT", b"patched")],
        ),
        &nested_container(
            "ASSETS",
            "SCRIPTS",
            &[Entry::plain("AIRFRAME.SCRIPT", b"main")],
        ),
        &[],
    );
    fs::remove_file(install.host_root().join("GOSDATA/Assets/crimson.rof"))
        .expect("the fixture container is removed");

    let mut builder = SessionBuilder::new(install_context(&install.with_registry()));
    let error = builder
        .mount_gos_chain(&install.with_registry())
        .expect_err("a chain without crimson.rof cannot be built");
    let rendered = error.to_string();
    assert!(
        rendered.contains("crimson.rof") && rendered.contains("step 2"),
        "the refusal names the step: {rendered}"
    );
    assert!(
        matches!(&error, cs_assets::vfs::GosError::Main { .. }),
        "{error:?}"
    );
}

// ------------------------------------------------------------- retail ---

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var_os("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

/// The original installation with its GOS chain mounted, and the installation
/// hashes every claim below is qualified by.
struct Retail {
    install_sha256: String,
    content_sha256: String,
    /// The chain as a registered installation's registry key builds it.
    registered: Chain,
    /// The same installation with the registry key absent.
    without_registry: Chain,
}

fn retail() -> Retail {
    let root = game_dir();
    let found = install::discover(&root).expect("the installation is discovered");
    let context = ResolveContext::new(fingerprint(&found.manifest));

    let registered_request = GosInstall::new(&root, ExePathOrigin::inspect_registry(&root));
    assert_eq!(
        registered_request.exe_path,
        ExePathOrigin::RegistryWithPatch(root.clone()),
        "this installation holds GOSDATA/Assets/crimptch.rof, which is the \
         patch the registry key's EXE Path names when the key is present"
    );
    let absent_request = GosInstall::new(&root, ExePathOrigin::RegistryKeyAbsent);

    let registered = mount_with(&context.clone(), &registered_request);
    let without_registry = mount_with(&context, &absent_request);
    assert!(
        registered.session.rejected().is_empty(),
        "{:?}",
        registered.session.rejected()
    );
    Retail {
        install_sha256: fingerprint(&found.manifest).to_hex(),
        content_sha256: content_fingerprint(&found.manifest).to_hex(),
        registered,
        without_registry,
    }
}

/// Every member of the main container, keyed by its source-relative logical
/// path, as the production mount indexes it.
fn main_members(chain: &Chain) -> BTreeMap<String, (String, u64, ContentHash)> {
    let mut members = BTreeMap::new();
    let step = chain
        .chain
        .step_of(GosSource::MainContainer)
        .expect("the main container is registered");
    for attempt in chain.session.mounts() {
        if attempt.id() != &step.mount {
            continue;
        }
        for (_, member) in attempt.members() {
            members.insert(
                member.spelling().logical_key(),
                (
                    member.spelling().as_str().to_owned(),
                    member.size_bytes(),
                    member.sha256().expect("directory members are hashed"),
                ),
            );
        }
    }
    members
}

/// On the original installation: the member sets the order is computed over,
/// and the answers the order then implies.
///
/// Pinned, and each assertion names the installation fingerprint it was
/// taken from, so a reader or mount change that would invalidate
/// `docs/findings/2026-10-05-f04-d-original-lookup-order.md` section D
/// fails here instead of quietly contradicting it.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f04_d_gos_registration_order_retail_order_over_the_measured_members() {
    let Retail {
        install_sha256,
        registered,
        without_registry,
        ..
    } = retail();

    // The measured container sizes of this installation.
    assert_eq!(
        registered
            .chain
            .step_of(GosSource::PatchContainer)
            .expect("the patch is registered")
            .members,
        1,
        "crimptch.rof holds exactly one member, installation {install_sha256}"
    );
    assert_eq!(
        registered
            .chain
            .step_of(GosSource::MainContainer)
            .expect("the main container is registered")
            .members,
        846,
        "crimson.rof holds 846 members, installation {install_sha256}"
    );
    assert_eq!(
        registered.chain.order(),
        ["patch_container", "main_container", "loose_ui_assets"]
    );
    assert_eq!(
        registered.chain.patch().label(),
        "registered",
        "installation {install_sha256}"
    );
    assert_eq!(
        without_registry.chain.order(),
        ["main_container", "loose_ui_assets"]
    );
    assert_eq!(
        without_registry.chain.patch().label(),
        "registry_key_absent"
    );

    let members = main_members(&registered);

    // The one key both containers hold: crimptch.rof's
    // `ASSETS/SCRIPTS/AIRFRAME.SCRIPT` (670 stored / 1641 decoded) shadows
    // crimson.rof's member of the same key (703 / 1813). Different stored
    // digests, so the shadowing is a real difference of bytes.
    let airframe = "assets/scripts/airframe.script";
    let (spelling, stored, digest) = members
        .get(airframe)
        .unwrap_or_else(|| panic!("{airframe} is in crimson.rof, installation {install_sha256}"));
    assert_eq!(spelling, "ASSETS/SCRIPTS/AIRFRAME.SCRIPT");
    assert_eq!(*stored, 703, "installation {install_sha256}");
    assert_eq!(
        digest.to_hex(),
        "e0ba801b72d6717a41d78558e21c455f8f5d72512d19203aa89b35af831a128f",
        "installation {install_sha256}"
    );

    let patch_step = registered
        .chain
        .step_of(GosSource::PatchContainer)
        .expect("the patch is registered");
    let patch_member = registered
        .session
        .mounts()
        .find(|mount| mount.id() == &patch_step.mount)
        .and_then(|mount| mount.member(&gos_key("ASSETS/SCRIPTS/AIRFRAME.SCRIPT").expect("key")))
        .expect("the patch holds it");
    assert_eq!(
        patch_member.spelling().as_str(),
        "ASSETS/SCRIPTS/AIRFRAME.SCRIPT"
    );
    assert_eq!(
        patch_member.size_bytes(),
        670,
        "installation {install_sha256}"
    );
    assert_eq!(
        patch_member
            .sha256()
            .expect("container members are hashed")
            .to_hex(),
        "05fa0136ba7add04722158cd94858d38ebfc9b077aa1d2417d0860822a8e58d6",
        "installation {install_sha256}"
    );

    let key = gos_key("ASSETS/SCRIPTS/AIRFRAME.SCRIPT").expect("a valid key");
    // Registry case A: the patch is registered first, so it serves the
    // request and the main container's member is a candidate that lost.
    let served = registered
        .session
        .resolve(&key)
        .expect("the patch serves the request");
    assert_eq!(served.resolved().mount.as_str(), PATCH_MOUNT_ID);
    assert_eq!(
        attempts(&served),
        [
            (PATCH_MOUNT_ID.to_owned(), AttemptOutcome::Selected),
            (MAIN_MOUNT_ID.to_owned(), AttemptOutcome::Candidate),
            (LOOSE_MOUNT_ID.to_owned(), AttemptOutcome::Miss),
        ]
    );
    assert_eq!(
        served.resolved().trace.order,
        LookupOrderStatus {
            order: LookupOrder::GosRegistration,
            status: GOS_ORDER_STATUS,
        }
    );
    assert_ne!(
        served.resolved().trace.order.status,
        ClaimStatus::VerifiedOriginal
    );

    // Registry case B: no key, no patch, and the *same* request is served by
    // crimson.rof. This is the pair of cases the order has two answers for,
    // both measured here over the same files.
    let served = without_registry
        .session
        .resolve(&key)
        .expect("the main container serves the request");
    assert_eq!(served.resolved().mount.as_str(), MAIN_MOUNT_ID);
    assert_eq!(
        attempts(&served),
        [
            (MAIN_MOUNT_ID.to_owned(), AttemptOutcome::Selected),
            (LOOSE_MOUNT_ID.to_owned(), AttemptOutcome::Miss),
        ]
    );

    // The case-only pairs: the container stores `ASSETS/GRAPHICS/ARIAL8.TGA`
    // and `FONT.TGA` with an uppercase file name, the loose tree stores the
    // same two images with a lowercase one. Both spellings are measured to
    // exist on this installation, so this is the case that depends on the
    // unmeasured matching rule (#693): the two rules are both exercised and
    // neither is presented as the original's.
    let exact_root = game_dir();
    let exact = mount_with(
        &bare_context(),
        &GosInstall::new(&exact_root, ExePathOrigin::inspect_registry(&exact_root))
            .with_name_match(GosNameMatch::ExactSpelling),
    );
    for name in ["ARIAL8.TGA", "FONT.TGA"] {
        let lowered = format!("assets/graphics/{}", name.to_ascii_lowercase());
        let spelled = format!("ASSETS/GRAPHICS/{}", name.to_ascii_lowercase());
        assert!(
            members.contains_key(&lowered),
            "crimson.rof holds {name}, installation {install_sha256}"
        );
        // The default rule folds, so the container answers and the loose copy
        // of the same image never comes up.
        let folding = registered
            .session
            .resolve(&gos_key(&lowered).expect("a valid key"))
            .expect("the request resolves under the default rule");
        assert_eq!(
            folding.resolved().mount.as_str(),
            MAIN_MOUNT_ID,
            "{name} is served by the container under the folding rule, installation \
             {install_sha256}"
        );
        // Under the exact rule the container's uppercase name is not the name
        // that was asked, and the lowercase request matches nothing at all;
        // only the loose spelling is answered, and by the loose file.
        assert!(
            matches!(
                exact
                    .session
                    .resolve(&gos_key(&lowered).expect("a valid key")),
                Err(ResolveError::NotFound { .. })
            ),
            "under the exact rule no source spells {name} the way it was asked, installation \
             {install_sha256}"
        );
        let exact_served = exact
            .session
            .resolve(&gos_key(&spelled).expect("a valid key"))
            .expect("the loose spelling resolves under the exact rule");
        assert_eq!(
            exact_served.resolved().mount.as_str(),
            LOOSE_MOUNT_ID,
            "{name} is served by the loose file under the exact rule, installation \
             {install_sha256}"
        );
        assert_eq!(
            exact_served.resolved().span.member_key(),
            Some(spelled.as_str())
        );
    }

    // Everything in `crimson.rof` that no earlier source holds is answered by
    // it, and the whole key space of the chain is the union of its members
    // and the loose files' — so the order is exercised over every member,
    // not only the two named cases.
    let loose_step = registered
        .chain
        .step_of(GosSource::LooseUiAssets)
        .expect("the loose directory is registered");
    let loose: BTreeSet<String> = registered
        .session
        .mounts()
        .find(|mount| mount.id() == &loose_step.mount)
        .map(|mount| {
            mount
                .members()
                .map(|(_, member)| member.spelling().logical_key())
                .collect()
        })
        .expect("the loose directory is registered");

    let mut union: BTreeSet<String> = members.keys().cloned().collect();
    union.extend(loose.iter().cloned());
    assert_eq!(
        union.len(),
        members.len()
            + loose
                .iter()
                .filter(|key| !members.contains_key(*key))
                .count(),
        "the chain's key space is the union of its sources"
    );
    assert!(
        loose.iter().any(|key| members.contains_key(key)),
        "the case-only loose pairs are both in the key space, installation {install_sha256}"
    );

    for key in &union {
        let served = registered
            .session
            .resolve(&AssetKey::from_spelling(GOS_NAMESPACE, key, "default").expect("a valid key"))
            .unwrap_or_else(|error| panic!("{key} resolves: {error}"));
        assert!(matches!(
            served.resolved().trace.order.order,
            LookupOrder::GosRegistration
        ));
        assert!(
            matches!(
                served.resolved().mount.as_str(),
                PATCH_MOUNT_ID | MAIN_MOUNT_ID | LOOSE_MOUNT_ID
            ),
            "{key} is served by a registered source: {:?}",
            served.resolved().mount
        );
    }
}

/// Reading a GOS-resolved member goes through the ordinary VFS read path and
/// yields the bytes of the origin the order chose: on this installation the
/// patch's `ASSETS/SCRIPTS/AIRFRAME.SCRIPT`, decoded, not the main
/// container's copy of the same name.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f04_d_gos_registration_order_retail_reads_the_patch_the_order_chose() {
    let Retail {
        registered,
        without_registry,
        ..
    } = retail();
    let key = gos_key("ASSETS/SCRIPTS/AIRFRAME.SCRIPT").expect("a valid key");

    let asset = registered
        .session
        .resolve(&key)
        .expect("the patch serves it");
    let from_patch = registered
        .chain
        .read(&registered.session, &asset)
        .expect("the served member reads");
    assert_ne!(from_patch, Vec::<u8>::new(), "the patch member has content");

    // The same request under the registry-absent case reads the main
    // container's member, which is a different length, so the two answers are
    // distinguishable rather than interchangeable.
    let asset = without_registry
        .session
        .resolve(&key)
        .expect("the main container serves it");
    let from_main = without_registry
        .chain
        .read(&without_registry.session, &asset)
        .expect("the served member reads");
    assert_ne!(
        from_main.len(),
        from_patch.len(),
        "the two containers hold different bytes for this name"
    );

    // The span of each answer names exactly one installation and container.
    assert_eq!(
        asset.resolved().span.install_sha256().to_hex(),
        registered.session.context().installation.to_hex()
    );
    assert_eq!(
        asset.resolved().span.container_path(),
        "GOSDATA/Assets/crimson.rof"
    );
}

// ------------------------------------------------------ evidence harness ---

/// Evidence-report harness for task #686 (`docs/contracts/CLI-EVIDENCE.md`,
/// schema `schemas/evidence.schema.json`). Not an acceptance test: it fails
/// loudly when its inputs are missing. `CS_EVIDENCE_REVIEWER` names the agent
/// that ran it and is recorded in the report; it is not baked in, because the
/// reviewer regenerates the report on the rebased commit. Run from the
/// workspace root, after the acceptance suite, exactly as:
///
/// 1. ```sh
///    mkdir -p private/evidence/T686
///    cargo test --workspace --locked -- accept_f04_d_gos_registration_order_ --include-ignored \
///      2>&1 | tee private/evidence/T686/cargo-test.log
///    ```
///    (record the exit status of `cargo test`, e.g. with `pipefail`.)
/// 2. ```sh
///    CS_EVIDENCE_DIR=private/evidence/T686 \
///    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
///    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f04_d_gos_registration_order_ --include-ignored" \
///    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
///    CS_EVIDENCE_REVIEWER="<agent running this harness>" \
///      cargo test --locked -p cs_assets --test accept_f04_d_gos_registration_order -- evidence_report_t686 --ignored
///    ```
/// 3. ```sh
///    python3 tools/validate_evidence.py private/evidence/T686/acceptance.json \
///      --artifact-root private/evidence/T686 --require-pass
///    ```
/// 4. Commit a copy of `acceptance.json` as `docs/findings/evidence/T686.json`.
///
/// Every field is derived from real inputs: the recorded log, production
/// discovery of `$CS_GAME_DIR`, the production GOS chains mounted from that
/// installation under both registry cases, the production reads of the
/// shadowed member (`gos-registration-order.json`: names, lengths and hashes
/// only), `rustc --version` and `Cargo.lock`.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_t686_writes_the_acceptance_report() {
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
    let mut retail_tests = 0;
    for name in [
        format!("{PREFIX}retail_order_over_the_measured_members"),
        format!("{PREFIX}retail_reads_the_patch_the_order_chose"),
    ] {
        assert_eq!(
            suite
                .assertions
                .iter()
                .find(|(seen, _)| *seen == name)
                .map(|(_, status)| *status),
            Some("pass"),
            "{name} must have run and passed (step 1 needs --include-ignored and CS_GAME_DIR)"
        );
        retail_tests += 1;
    }
    assert_eq!(retail_tests, 2);
    assert!(
        suite
            .assertions
            .iter()
            .any(|(name, _)| !name.starts_with(&format!("{PREFIX}retail_"))),
        "synthetic task tests must be present alongside the retail ones"
    );

    let retail = retail();
    let order_path = evidence_dir.join("gos-registration-order.json");
    fs::write(
        &order_path,
        registration_order_json(&candidate_tree, &retail),
    )
    .unwrap_or_else(|error| panic!("write {}: {error}", order_path.display()));
    let artifacts = [artifact(&log_path, "log"), artifact(&order_path, "json")];

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"T686\",\n\
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
        unknowns()
            .iter()
            .map(|unknown| jstr(unknown))
            .collect::<Vec<_>>()
            .join(", "),
        jstr(&reviewer),
        jstr(
            "acceptance suite run locally with the retail capability; this harness derives every field \
             from the recorded log, production discovery of $CS_GAME_DIR, the production GOS chains \
             mounted from that installation under both registry cases, production reads of the shadowed \
             member, rustc and Cargo.lock; the order stays inferred and the registry state of this \
             machine is not claimed"
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

/// The measured registration order of the original installation: both
/// registry cases, the members each source contributes, and what each case
/// answers for the one key both containers hold. Names, lengths and hashes
/// only — never original bytes.
fn registration_order_json(candidate_tree: &str, retail: &Retail) -> String {
    let case = |chain: &Chain| {
        let steps: Vec<String> = chain
            .chain
            .steps()
            .iter()
            .map(|step| {
                format!(
                    "{{\"source\": {}, \"mount\": {}, \"container\": {}, \"members\": {}}}",
                    jstr(step.source.label()),
                    jstr(step.mount.as_str()),
                    jstr(&step.container.to_string_lossy()),
                    step.members
                )
            })
            .collect();
        format!(
            "{{\"patch\": {}, \"patch_detail\": {}, \"order\": [{}], \"unregistered\": [{}], \
             \"name_match\": {}, \"order_status\": {}}}",
            jstr(chain.chain.patch().label()),
            jstr(&chain.chain.patch().to_string()),
            steps.join(", "),
            chain
                .chain
                .unregistered()
                .iter()
                .map(|source| jstr(source.label()))
                .collect::<Vec<_>>()
                .join(", "),
            jstr(chain.chain.name_match().label()),
            jstr(chain.chain.order_status().label()),
        )
    };

    let key = gos_key("ASSETS/SCRIPTS/AIRFRAME.SCRIPT").expect("a valid key");
    let answer = |chain: &Chain| {
        let asset = chain
            .session
            .resolve(&key)
            .expect("the shadowed key resolves in both cases");
        let attempts: Vec<String> = asset
            .resolved()
            .trace
            .attempts
            .iter()
            .map(|attempt| {
                format!(
                    "{{\"mount\": {}, \"container\": {}, \"outcome\": {}}}",
                    jstr(attempt.mount.as_str()),
                    jstr(&attempt.container),
                    jstr(attempt.outcome.label())
                )
            })
            .collect();
        let bytes = chain
            .chain
            .read(&chain.session, &asset)
            .expect("the served member reads");
        format!(
            "{{\"mount\": {}, \"container\": {}, \"member\": {}, \"decoded_len\": {}, \
             \"decoded_sha256\": {}, \"order\": {}, \"order_status\": {}, \"attempts\": [{}]}}",
            jstr(asset.resolved().mount.as_str()),
            jstr(asset.resolved().span.container_path()),
            jstr(asset.resolved().span.member_key().unwrap_or_default()),
            bytes.len(),
            jstr(&sha256(&bytes).to_hex()),
            jstr(asset.resolved().trace.order.order.label()),
            jstr(asset.resolved().trace.order.status.label()),
            attempts.join(", "),
        )
    };

    let members = main_members(&retail.registered);
    let mut keys: BTreeMap<&str, (String, u64, String)> = BTreeMap::new();
    for key in [
        "assets/scripts/airframe.script",
        "assets/graphics/arial8.tga",
        "assets/graphics/font.tga",
    ] {
        if let Some((spelling, len, digest)) = members.get(key) {
            keys.insert(key, (spelling.clone(), *len, digest.to_hex()));
        }
    }
    let members_json: Vec<String> = keys
        .iter()
        .map(|(key, (spelling, len, digest))| {
            format!(
                "{{\"key\": {}, \"spelling\": {}, \"stored_len\": {len}, \"stored_sha256\": {}}}",
                jstr(key),
                jstr(spelling),
                jstr(digest)
            )
        })
        .collect();

    format!(
        "{{\n\
         \x20\"task_id\": \"T686\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"install_sha256\": {},\n\
         \x20\"registry_case\": \"not readable on this host; both cases are measured over the same \
         installation by stating ExePathOrigin\",\n\
         \x20\"order_provenance\": \"static analysis of crimson.decrypted.exe and roffile.dll, not an \
         original run\",\n\
         \x20\"key_space\": {{\"crimson_rof_members\": {}, \"chain_keys\": {}}},\n\
         \x20\"crimson_rof_members_of_interest\": [\n  {}\n ],\n\
         \x20\"registry_present\": {},\n\
         \x20\"registry_absent\": {},\n\
         \x20\"answers\": {{\"registry_present\": {}, \"registry_absent\": {}}}\n\
         }}\n",
        jstr(candidate_tree),
        jstr(&iso_utc_now()),
        jstr(&retail.install_sha256),
        members.len(),
        members.len() + 16,
        members_json.join(",\n  "),
        case(&retail.registered),
        case(&retail.without_registry),
        answer(&retail.registered),
        answer(&retail.without_registry),
    )
}

/// What this task does **not** know, recorded in the report as strings.
///
/// Each names the affected content and what would resolve it. They are here
/// because the order is code-derived and two of its inputs are not measurable
/// from files, and a report that claimed otherwise would be the failure this
/// workspace's evidence policy forbids.
fn unknowns() -> Vec<String> {
    vec![
        "gos-name-matching: MetaOpenFile's case-matching rule decides whether a request for \
         ASSETS/GRAPHICS/ARIAL8.TGA or FONT.TGA gets the crimson.rof member or the loose \
         GOSDATA copy of the same image; it is unmeasured (#693), so it is an explicit chain \
         input (GosNameMatch) and both rules are implemented and exercised. Resolved by #693, or \
         by an owner-supplied original run."
            .to_owned(),
        "gos-registry-key: HKLM\\SOFTWARE\\Microsoft\\Microsoft Games\\Crimson Skies\\1.0 is not \
         readable on this host, so the registry state of this machine is not claimed; both \
         registry cases (patch registered, patch skipped) are measured over the same \
         installation files by stating ExePathOrigin. Resolved by the owner supplying the key's \
         EXE Path value or an original-run capture."
            .to_owned(),
        "gos-order-provenance: the registration order crimptch.rof -> crimson.rof -> loose \
         GOSDATA -> current directory is derived from static analysis of crimson.decrypted.exe \
         and roffile.dll, not from running the original engine, so GOS_ORDER_STATUS stays \
         inferred. Resolved only by an owner-supplied original run."
            .to_owned(),
    ]
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
    let day_of_year = day_of_era - (year_of_era * 365 + year_of_era / 4 - year_of_era / 100);
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
