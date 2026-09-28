//! Member-level collisions between two mounted ROF archives (task #345,
//! F04-D follow-up 2 in
//! `docs/findings/2026-09-28-f04-d-observed-collisions-and-async-cancel.md`).
//!
//! F04-D compared collisions between *files*. The small patch archive
//! `GOSDATA/ASSETS/crimptch.rof` holds members whose keys `crimson.rof`
//! also holds; that overlap only exists once both archives' members are
//! mounted (F05-C). Both are mounted here as **retail** mounts with a
//! designed scope — `crimson.rof` as the shared base, `crimptch.rof` as a
//! patch overlay, both unbound to any world — and every member collision is
//! looked up under no world and under every world group through
//! `ContentSession::collision_report`, the lookup content sessions use.
//!
//! Which archive the original engine prefers is **not measured**. A lookup
//! that the designed order alone would decide between different bytes must
//! therefore be refused with `ResolveError::UnmeasuredOrder` (spec F04
//! non-negotiable behavior 2); nothing here assumes the patch wins.
//!
//! The synthetic tests author ROF containers under the system temporary
//! directory (`common::TempTree`) and mount them through the production
//! `cs_assets::rof::mount_rof_into`. The `retail_` test reads `$CS_GAME_DIR`
//! read-only and fails loudly without it; CI skips it.
//! `evidence_report_t345_writes_the_acceptance_report` is the evidence
//! harness (`docs/contracts/CLI-EVIDENCE.md`), not an acceptance test.

mod common;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use common::TempTree;
use cs_assets::install::{self, content_fingerprint, fingerprint, sha256};
use cs_assets::rof::{RofSource, mount_rof_into};
use cs_assets::vfs::{
    CollisionComparison, CollisionReport, CollisionVerdict, ContentSession, INSTALL_NAMESPACE,
    LookupOutcome, MountBuilder, ResolveError, SessionBuilder,
};
use cs_formats::{DIRECTORY_HEADER_BYTES, FLAG_COMPRESSED, FLAG_DIRECTORY, RECORD_BYTES};
use cs_types::asset_id::{
    AssetKey, MountId, MountNamespace, PrecedenceClass, ResolveContext, WorldGroup,
};
use cs_types::evidence::ClaimStatus;

/// The acceptance prefix of this task.
const PREFIX: &str = "accept_f04_d_rof_member_collisions_";

/// The two retail containers, spelled as the installation spells them.
const BASE_CONTAINER: &str = "GOSDATA/ASSETS/crimson.rof";
const PATCH_CONTAINER: &str = "GOSDATA/ASSETS/crimptch.rof";

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

/// `[root: <directory>/][<directory>: entries…][payloads…]` — the shape of
/// the retail overlap (`ASSETS/SCRIPTS/<name>` in both archives), one
/// directory deep.
fn container(directory: &str, entries: &[Entry<'_>]) -> Vec<u8> {
    let root_names = names(&[directory]);
    let root_len = DIRECTORY_HEADER_BYTES + RECORD_BYTES + root_names.len();
    let entry_names: Vec<&str> = entries.iter().map(|entry| entry.name).collect();
    let sub_names = names(&entry_names);
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

/// Both archives mounted into one session, plus the sources that read them.
struct Pair {
    session: ContentSession,
    base: RofSource,
    patch: RofSource,
}

fn mount_pair(context: ResolveContext, base_path: &Path, patch_path: &Path) -> Pair {
    let mut builder = SessionBuilder::new(context);
    let base = mount_rof_into(
        &mut builder,
        rof_mount(BASE_CONTAINER, PrecedenceClass::Shared),
        base_path,
    )
    .expect("the base archive mounts");
    let patch = mount_rof_into(
        &mut builder,
        rof_mount(PATCH_CONTAINER, PrecedenceClass::Patch),
        patch_path,
    )
    .expect("the patch archive mounts");
    Pair {
        session: builder.open(),
        base,
        patch,
    }
}

/// Writes both authored archives and mounts them.
fn synthetic_pair(label: &str, base: &[u8], patch: &[u8]) -> (TempTree, Pair) {
    let tree = TempTree::new(label);
    tree.write("GOSDATA/ASSETS/crimson.rof", base);
    tree.write("GOSDATA/ASSETS/crimptch.rof", patch);
    let pair = mount_pair(
        ResolveContext::new(sha256(label.as_bytes())),
        &tree.root().join(BASE_CONTAINER),
        &tree.root().join(PATCH_CONTAINER),
    );
    (tree, pair)
}

fn key(spelling: &str) -> AssetKey {
    AssetKey::from_spelling(INSTALL_NAMESPACE, spelling, "default").expect("key is valid")
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
        .map(|world| WorldGroup::new(world).expect("world is valid"))
        .collect()
}

fn comparison<'a>(report: &'a CollisionReport, file_name: &str) -> &'a CollisionComparison {
    report
        .comparisons
        .iter()
        .find(|comparison| comparison.collision.file_name == file_name)
        .unwrap_or_else(|| panic!("{file_name} is a collision"))
}

/// Asserts that every lookup of `comparison` is refused as decided only by
/// the unmeasured order, with the patch selected and the base shadowed.
fn assert_blocked_everywhere(comparison: &CollisionComparison, patch: &MountId, base: &MountId) {
    assert_eq!(comparison.verdict, CollisionVerdict::Conflicting);
    assert!(!comparison.lookups.is_empty());
    for lookup in &comparison.lookups {
        match &lookup.outcome {
            LookupOutcome::Blocked { selected, shadowed } => {
                assert_eq!(&selected.mount, patch, "designed order selects the patch");
                assert_eq!(selected.precedence, PrecedenceClass::Patch);
                assert_eq!(shadowed.len(), 1, "{shadowed:?}");
                assert_eq!(&shadowed[0].mount, base);
                assert_ne!(shadowed[0].sha256, selected.sha256);
            }
            other => panic!(
                "{} of {} under context {}: expected blocked, got {other}",
                comparison.collision.members[lookup.member].spelling,
                comparison.collision.members[lookup.member].container,
                lookup.context
            ),
        }
    }
}

// ----------------------------------------------------------- synthetic ---

/// The retail shape: the patch archive holds `SCRIPTS/AIRFRAME.SCRIPT`
/// with other bytes than the base archive's member of the same key. The
/// designed order would serve the patch, but nothing measured says the
/// original does, so the lookup is refused under every context — through
/// the session's own `resolve` as well as the collision report — and a
/// member only the base holds still resolves.
#[test]
fn accept_f04_d_rof_member_collisions_different_bytes_are_blocked() {
    let base = container(
        "SCRIPTS",
        &[
            Entry::plain("AIRFRAME.SCRIPT", b"base airframe script"),
            Entry::plain("WEAPONS.SCRIPT", b"base weapons script"),
        ],
    );
    let patch = container(
        "SCRIPTS",
        &[Entry::plain("AIRFRAME.SCRIPT", b"patched airframe script")],
    );
    let (_tree, pair) = synthetic_pair("t345-different", &base, &patch);
    let (base_id, patch_id) = (pair.base.mount_id().clone(), pair.patch.mount_id().clone());

    match pair.session.resolve(&key("SCRIPTS/AIRFRAME.SCRIPT")) {
        Err(ResolveError::UnmeasuredOrder {
            selected, shadowed, ..
        }) => {
            assert_eq!(selected.mount, patch_id);
            assert_eq!(selected.container, PATCH_CONTAINER);
            assert_eq!(shadowed.len(), 1);
            assert_eq!(shadowed[0].mount, base_id);
            assert_eq!(shadowed[0].container, BASE_CONTAINER);
            assert_eq!(shadowed[0].member_spelling, "SCRIPTS/AIRFRAME.SCRIPT");
        }
        other => panic!("the patch must not win by the designed order alone: {other:?}"),
    }
    // Case and separators fold exactly as for files: the same key.
    assert!(matches!(
        pair.session.resolve(&key("scripts\\airframe.script")),
        Err(ResolveError::UnmeasuredOrder { .. })
    ));
    let only_base = pair
        .session
        .resolve(&key("SCRIPTS/WEAPONS.SCRIPT"))
        .expect("a member only the base holds resolves");
    assert_eq!(only_base.resolved().mount, base_id);

    let report = pair
        .session
        .collision_report(&contexts(pair.session.context(), &synthetic_worlds()));
    assert_eq!(report.precedence_status, ClaimStatus::Designed);
    assert_eq!(report.contexts.len(), 3);
    assert_eq!(report.comparisons.len(), 1, "weapons.script is unique");
    let airframe = comparison(&report, "airframe.script");
    assert_eq!(airframe.collision.members.len(), 2);
    assert_eq!(airframe.collision.distinct_digests(), 2);
    assert_eq!(airframe.lookups.len(), 2 * 3);
    assert_blocked_everywhere(airframe, &patch_id, &base_id);
    assert_eq!(report.conflicting().count(), 1);
}

/// Identical stored bytes under one key: the designed order decides only
/// which origin is reported, not which bytes, so the lookup resolves and
/// the collision is `shadowed_by_identical_bytes`.
#[test]
fn accept_f04_d_rof_member_collisions_identical_digest_resolves() {
    let base = container(
        "SCRIPTS",
        &[Entry::plain("AIRFRAME.SCRIPT", b"one airframe script")],
    );
    let patch = container(
        "SCRIPTS",
        &[Entry::plain("AIRFRAME.SCRIPT", b"one airframe script")],
    );
    let (_tree, pair) = synthetic_pair("t345-identical", &base, &patch);
    let (base_id, patch_id) = (pair.base.mount_id().clone(), pair.patch.mount_id().clone());

    let asset = pair
        .session
        .resolve(&key("SCRIPTS/AIRFRAME.SCRIPT"))
        .expect("identical bytes resolve");
    let read = pair
        .patch
        .read(&asset.resolved().key)
        .expect("the served member reads");
    assert_eq!(read.data, b"one airframe script");

    let report = pair
        .session
        .collision_report(&contexts(pair.session.context(), &synthetic_worlds()));
    let airframe = comparison(&report, "airframe.script");
    assert_eq!(airframe.collision.distinct_digests(), 1);
    assert_eq!(airframe.verdict, CollisionVerdict::ShadowedByIdenticalBytes);
    for lookup in &airframe.lookups {
        let member = &airframe.collision.members[lookup.member];
        if member.mount == patch_id {
            assert_eq!(lookup.outcome, LookupOutcome::Own);
        } else {
            assert_eq!(member.mount, base_id);
            assert_eq!(
                lookup.outcome,
                LookupOutcome::Other {
                    mount: patch_id.clone(),
                    spelling: "SCRIPTS/AIRFRAME.SCRIPT".to_owned(),
                    same_bytes: true,
                }
            );
        }
    }
    assert_eq!(report.conflicting().count(), 0);
}

/// The mount digests the *stored* extent (F05-C/F05-D). Two members that
/// decode to the same bytes but are stored differently — one plain, one a
/// zlib stream — therefore have different digests, and the lookup stays
/// blocked: equal content is never inferred from a decode the lookup did
/// not make, and the conservative answer is the refusal.
#[test]
fn accept_f04_d_rof_member_collisions_equal_content_stored_differently_stays_blocked() {
    let payload = b"airframe script stored two ways";
    let base = container("SCRIPTS", &[Entry::plain("AIRFRAME.SCRIPT", payload)]);
    let patch = container("SCRIPTS", &[Entry::zlib_stored("AIRFRAME.SCRIPT", payload)]);
    let (_tree, pair) = synthetic_pair("t345-stored-differently", &base, &patch);
    let spelling = key("SCRIPTS/AIRFRAME.SCRIPT");

    let base_info = pair.base.member(&spelling).expect("base member");
    let patch_info = pair.patch.member(&spelling).expect("patch member");
    assert!(patch_info.compressed && !base_info.compressed);
    assert_ne!(base_info.sha256, patch_info.sha256, "stored digests differ");
    assert_eq!(
        pair.base.read(&spelling).expect("reads").data,
        pair.patch.read(&spelling).expect("decodes").data,
        "the decoded bytes are equal"
    );

    assert!(matches!(
        pair.session.resolve(&spelling),
        Err(ResolveError::UnmeasuredOrder { .. })
    ));
    let report = pair
        .session
        .collision_report(&contexts(pair.session.context(), &synthetic_worlds()));
    assert_blocked_everywhere(
        comparison(&report, "airframe.script"),
        pair.patch.mount_id(),
        pair.base.mount_id(),
    );
}

/// A file name both archives hold under *different* directories is a
/// name collision, not a key collision: each member resolves to itself and
/// the order decides nothing.
#[test]
fn accept_f04_d_rof_member_collisions_other_directories_are_distinct_by_path() {
    let base = container(
        "SCRIPTS",
        &[Entry::plain("AIRFRAME.SCRIPT", b"base airframe script")],
    );
    let patch = container(
        "PATCH",
        &[Entry::plain("AIRFRAME.SCRIPT", b"patched airframe script")],
    );
    let (_tree, pair) = synthetic_pair("t345-other-directory", &base, &patch);

    let report = pair
        .session
        .collision_report(&contexts(pair.session.context(), &synthetic_worlds()));
    let airframe = comparison(&report, "airframe.script");
    assert_eq!(airframe.verdict, CollisionVerdict::DistinctByPath);
    assert!(
        airframe
            .lookups
            .iter()
            .all(|lookup| lookup.outcome == LookupOutcome::Own)
    );
    assert_eq!(
        pair.session
            .resolve(&key("SCRIPTS/AIRFRAME.SCRIPT"))
            .expect("resolves")
            .resolved()
            .mount,
        *pair.base.mount_id()
    );
    assert_eq!(
        pair.session
            .resolve(&key("PATCH/AIRFRAME.SCRIPT"))
            .expect("resolves")
            .resolved()
            .mount,
        *pair.patch.mount_id()
    );
}

// ------------------------------------------------------------- retail ---

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var_os("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

/// The retail installation, both archives mounted, and every context.
struct Retail {
    install_sha256: String,
    content_sha256: String,
    pair: Pair,
    report: CollisionReport,
}

fn retail() -> Retail {
    let root = game_dir();
    let found = install::discover(&root).expect("the installation is discovered");
    assert!(
        found.diagnosis.world_groups.len() >= 2,
        "a retail installation has several world groups"
    );
    let context = ResolveContext::new(fingerprint(&found.manifest));
    let pair = mount_pair(
        context.clone(),
        &root.join(BASE_CONTAINER),
        &root.join(PATCH_CONTAINER),
    );
    assert!(
        pair.session.rejected().is_empty(),
        "{:?}",
        pair.session.rejected()
    );
    let worlds: Vec<WorldGroup> = found
        .diagnosis
        .world_groups
        .iter()
        .map(|group| WorldGroup::from_relative(group.clone()))
        .collect();
    let report = pair.session.collision_report(&contexts(&context, &worlds));
    Retail {
        install_sha256: fingerprint(&found.manifest).to_hex(),
        content_sha256: content_fingerprint(&found.manifest).to_hex(),
        pair,
        report,
    }
}

/// The keys both archives hold, computed from the two sources' own member
/// lists — independently of the collision report.
fn shared_keys(pair: &Pair) -> BTreeSet<String> {
    let base: BTreeSet<String> = pair
        .base
        .members()
        .map(|member| member.spelling.to_ascii_lowercase())
        .collect();
    pair.patch
        .members()
        .map(|member| member.spelling.to_ascii_lowercase())
        .filter(|spelling| base.contains(spelling))
        .collect()
}

/// Every member collision between `crimptch.rof` and `crimson.rof` on the
/// original installation, under no world and every world group: each key
/// both archives hold is refused as decided only by the unmeasured order
/// when their stored bytes differ (and resolves when they are identical);
/// every other file-name collision — same name, other directory, inside
/// one archive or across both — resolves each member by its own path.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f04_d_rof_member_collisions_retail_patch_over_base_until_measured() {
    let Retail { pair, report, .. } = retail();
    assert_eq!(report.precedence_status, ClaimStatus::Designed);
    assert!(report.contexts.len() >= 3);
    let base_id = pair.base.mount_id().clone();
    let patch_id = pair.patch.mount_id().clone();
    assert!(pair.base.member_count() > 0 && pair.patch.member_count() > 0);

    let shared = shared_keys(&pair);
    assert!(
        !shared.is_empty(),
        "crimptch.rof overlays at least one crimson.rof member"
    );

    let mut seen = BTreeSet::new();
    for comparison in &report.comparisons {
        let members = &comparison.collision.members;
        for (index, member) in members.iter().enumerate() {
            let logical = member.spelling.to_ascii_lowercase();
            if !shared.contains(&logical) {
                // Not a key both archives hold: its own path decides.
                for lookup in comparison.lookups.iter().filter(|l| l.member == index) {
                    assert_eq!(
                        lookup.outcome,
                        LookupOutcome::Own,
                        "{} in {}",
                        member.spelling,
                        member.container
                    );
                }
                continue;
            }
            seen.insert(logical.clone());
            let spelling = key(&member.spelling);
            let base = pair.base.member(&spelling).expect("base holds it");
            let patch = pair.patch.member(&spelling).expect("patch holds it");
            for lookup in comparison.lookups.iter().filter(|l| l.member == index) {
                if base.sha256 == patch.sha256 {
                    assert!(
                        matches!(
                            &lookup.outcome,
                            LookupOutcome::Own
                                | LookupOutcome::Other {
                                    same_bytes: true,
                                    ..
                                }
                        ),
                        "{logical}: identical bytes resolve, got {}",
                        lookup.outcome
                    );
                } else {
                    match &lookup.outcome {
                        LookupOutcome::Blocked { selected, shadowed } => {
                            assert_eq!(selected.mount, patch_id);
                            assert_eq!(shadowed.len(), 1);
                            assert_eq!(shadowed[0].mount, base_id);
                        }
                        other => panic!("{logical}: expected blocked, got {other}"),
                    }
                }
            }
            let resolved = pair.session.resolve(&spelling);
            if base.sha256 == patch.sha256 {
                assert!(resolved.is_ok(), "{logical}: {resolved:?}");
            } else {
                assert!(
                    matches!(resolved, Err(ResolveError::UnmeasuredOrder { .. })),
                    "{logical}: the patch must not win by the designed order alone"
                );
            }
        }
        let crosses = members
            .iter()
            .any(|member| shared.contains(&member.spelling.to_ascii_lowercase()));
        if !crosses {
            assert_eq!(
                comparison.verdict,
                CollisionVerdict::DistinctByPath,
                "{}",
                comparison.collision.file_name
            );
        }
    }
    assert_eq!(seen, shared, "the report covers every shared key");

    // Decoded bytes are measured next to the stored digest for the
    // findings; a stored-identical pair must decode identically.
    for logical in &shared {
        let spelling = key(logical);
        let base = pair.base.read(&spelling).expect("the base member reads");
        let patch = pair.patch.read(&spelling).expect("the patch member reads");
        if pair.base.member(&spelling).map(|m| m.sha256)
            == pair.patch.member(&spelling).map(|m| m.sha256)
        {
            assert_eq!(base.data, patch.data);
        }
    }
}

// ------------------------------------------------------ evidence harness ---

/// Evidence-report harness for task #345 (`docs/contracts/CLI-EVIDENCE.md`,
/// schema `schemas/evidence.schema.json`). Not an acceptance test: it fails
/// loudly when its inputs are missing. Run from the workspace root, after
/// the acceptance suite, exactly as:
///
/// 1. ```sh
///    mkdir -p private/evidence/T345
///    cargo test --workspace --locked -- accept_f04_d_rof_member_collisions_ --include-ignored \
///      2>&1 | tee private/evidence/T345/cargo-test.log
///    ```
///    (record the exit status of `cargo test`, e.g. with `pipefail`.)
/// 2. ```sh
///    CS_EVIDENCE_DIR=private/evidence/T345 \
///    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
///    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f04_d_rof_member_collisions_ --include-ignored" \
///    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
///      cargo test --locked -p cs_assets --test accept_f04_d_rof_member_collisions -- evidence_report_t345 --ignored
///    ```
/// 3. ```sh
///    python3 tools/validate_evidence.py private/evidence/T345/acceptance.json \
///      --artifact-root private/evidence/T345 --require-pass
///    ```
/// 4. Commit a copy of `acceptance.json` as `docs/findings/evidence/T345.json`.
///
/// Every field is derived from real inputs: the recorded log, production
/// discovery of `$CS_GAME_DIR`, the production collision comparison of the
/// two mounted archives and the production reads of every shared member
/// (`rof-member-collisions.json`: names, lengths and hashes only),
/// `rustc --version` and `Cargo.lock`.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_t345_writes_the_acceptance_report() {
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
    let retail_test = format!("{PREFIX}retail_patch_over_base_until_measured");
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
    let collisions_path = evidence_dir.join("rof-member-collisions.json");
    fs::write(
        &collisions_path,
        member_collisions_json(&candidate_tree, &retail),
    )
    .unwrap_or_else(|error| panic!("write {}: {error}", collisions_path.display()));
    let artifacts = [
        artifact(&log_path, "log"),
        artifact(&collisions_path, "json"),
    ];

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"T345\",\n\
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
        jstr(
            "claude-1 (implementing agent, self-check; the Rally reviewer regenerates this \
             report on the rebased commit)"
        ),
        jstr(
            "acceptance suite run locally with the retail capability; this harness derives \
             every field from the recorded log, production discovery of $CS_GAME_DIR, the \
             production collision comparison of both mounted ROF archives, production reads of \
             every shared member, rustc and Cargo.lock; the original lookup order stays \
             unmeasured (recorded in rof-member-collisions.json and docs/findings) and the \
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

/// The measured member collisions: every key both archives hold (both
/// members' stored and decoded lengths and digests, how each lookup
/// resolved) and a summary of every other file-name collision. Names,
/// lengths and hashes only — never original bytes.
fn member_collisions_json(candidate_tree: &str, retail: &Retail) -> String {
    let Retail { pair, report, .. } = retail;
    let shared = shared_keys(pair);
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

    let mut shared_json = Vec::new();
    let mut other_verdicts: BTreeMap<&str, u64> = BTreeMap::new();
    for comparison in &report.comparisons {
        let crossing: Vec<usize> = comparison
            .collision
            .members
            .iter()
            .enumerate()
            .filter(|(_, member)| shared.contains(&member.spelling.to_ascii_lowercase()))
            .map(|(index, _)| index)
            .collect();
        if crossing.is_empty() {
            *other_verdicts
                .entry(comparison.verdict.label())
                .or_default() += 1;
            continue;
        }
        for logical in crossing
            .iter()
            .map(|index| {
                comparison.collision.members[*index]
                    .spelling
                    .to_ascii_lowercase()
            })
            .collect::<BTreeSet<_>>()
        {
            let spelling = key(&logical);
            let side = |source: &RofSource| {
                let info = source.member(&spelling).expect("both hold it");
                let read = source.read(&spelling).expect("the member reads");
                format!(
                    "{{\"mount\": {}, \"container\": {}, \"spelling\": {}, \"id\": {}, \
                     \"compressed\": {}, \"stored_len\": {}, \"stored_sha256\": {}, \
                     \"decoded_len\": {}, \"decoded_sha256\": {}}}",
                    jstr(source.mount_id().as_str()),
                    jstr(source.container()),
                    jstr(&info.spelling),
                    info.id,
                    info.compressed,
                    info.stored_len,
                    jstr(&info.sha256.to_hex()),
                    read.decoded_len,
                    jstr(&sha256(&read.data).to_hex()),
                )
            };
            let mut outcomes: BTreeMap<&str, u64> = BTreeMap::new();
            for lookup in comparison.lookups.iter().filter(|lookup| {
                comparison.collision.members[lookup.member]
                    .spelling
                    .eq_ignore_ascii_case(&logical)
            }) {
                *outcomes.entry(lookup.outcome.label()).or_default() += 1;
            }
            let session_resolve = match pair.session.resolve(&spelling) {
                Ok(_) => "resolved",
                Err(ResolveError::UnmeasuredOrder { .. }) => "blocked_unmeasured_order",
                Err(ResolveError::Ambiguous { .. }) => "ambiguous",
                Err(ResolveError::NotFound { .. }) => "not_found",
            };
            shared_json.push(format!(
                "{{\"key\": {}, \"file_name\": {}, \"verdict\": {}, \"session_resolve\": {}, \
                 \"lookup_outcomes\": {{{}}}, \"base\": {}, \"patch\": {}}}",
                jstr(&logical),
                jstr(&comparison.collision.file_name),
                jstr(comparison.verdict.label()),
                jstr(session_resolve),
                outcomes
                    .iter()
                    .map(|(label, count)| format!("{}: {count}", jstr(label)))
                    .collect::<Vec<_>>()
                    .join(", "),
                side(&pair.base),
                side(&pair.patch),
            ));
        }
    }
    format!(
        "{{\n\
         \x20\"task_id\": \"T345\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"install_sha256\": {},\n\
         \x20\"layout\": \"crimson.rof shared retail, crimptch.rof patch retail, both unscoped (designed)\",\n\
         \x20\"precedence_status\": {},\n\
         \x20\"original_lookup_behavior\": \"unmeasured\",\n\
         \x20\"contexts\": [{}],\n\
         \x20\"members\": {{\"base\": {}, \"patch\": {}}},\n\
         \x20\"shared_key_count\": {},\n\
         \x20\"shared_keys\": [\n  {}\n ],\n\
         \x20\"other_file_name_collisions\": {{{}}}\n\
         }}\n",
        jstr(candidate_tree),
        jstr(&iso_utc_now()),
        jstr(&retail.install_sha256),
        jstr(report.precedence_status.label()),
        contexts.join(", "),
        pair.base.member_count(),
        pair.patch.member_count(),
        shared.len(),
        shared_json.join(",\n  "),
        other_verdicts
            .iter()
            .map(|(label, count)| format!("{}: {count}", jstr(label)))
            .collect::<Vec<_>>()
            .join(", "),
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
