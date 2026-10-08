//! Evidence-report harness for task F47-D (`docs/contracts/CLI-EVIDENCE.md`,
//! schema `schemas/evidence.schema.json`).
//!
//! This test is deliberately **not** named `accept_f47_d_*`: it is not part of
//! the acceptance suite, and it fails loudly when its inputs are missing
//! instead of passing vacuously. Run from the workspace root, after the
//! acceptance suite, exactly as:
//!
//! 1. ```sh
//!    mkdir -p private/evidence/F47-D
//!    CS_EVIDENCE_DIR=private/evidence/F47-D \
//!      cargo test --workspace --locked -- accept_f47_d_ --include-ignored \
//!      2>&1 | tee private/evidence/F47-D/cargo-test.log
//!    ```
//!    (record the pipeline's exit status; it is passed to this harness as
//!    `CS_EVIDENCE_EXIT_CODE`. `CS_EVIDENCE_DIR` is where the retail GPU test
//!    writes its page captures.)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F47-D \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f47_d_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!      cargo test --locked --test evidence_report_f47_d -- --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/F47-D/acceptance.json \
//!      --artifact-root private/evidence/F47-D --require-pass
//!    ```
//! 4. Commit a copy of `acceptance.json` as `docs/findings/evidence/F47-D.json`.
//!
//! Every field of the report is derived here from real inputs: the recorded
//! test log, production discovery of `$CS_GAME_DIR`, the production discovery
//! and audit over that installation, the production baseline's own report
//! (the consumer trace), `rustc --version` and `Cargo.lock`. The only texts
//! this file holds are the [`review_identity`] literal the committed
//! `docs/findings/evidence/F47-D.json` carries (a reviewing agent replaces it
//! with their own through `CS_EVIDENCE_REVIEW`) and the product-coverage
//! limitations quoted into `review.method`; everything else is measured.
//!
//! The discovery is cross-checked against an **independent derivation**: a
//! second mount of the same archive that re-parses `ASSETS/SCRAPBOOK.CSV`
//! with the production keyed-list reader, re-groups the records by their own
//! entry keys and re-joins their pictures against the container's member
//! spellings — it never calls `DiscoveredScrapbook::discover`. The GPU half is
//! checked by the captures themselves: one PNG per discovered page, hashed
//! here, each written by the retail capture test through
//! `cs_app::ui::front_end::capture_artwork` on the real adapter.
//!
//! `unknowns` is `[]` and the report validates with `--require-pass`: the
//! **task's** acceptance is complete — the audit runs, reports and pins what
//! the installation holds, and every selected test passed. The product
//! incompleteness the audit *measured* (the runtime declares none of the 461
//! items; no original unlock, replay-target or memento field exists to read)
//! is a different state and is moved, never deleted (the 2026-09-28 owner
//! directive): it is named in `UNKNOWN_LIMITATIONS` below, in
//! `review.method`, in the hashed `scrapbook-audit.json` artifact, in
//! `docs/findings/2026-10-08-f47-d-retail-scrapbook-audit.md` and in the
//! follow-up tasks. A failing run produces a failing report, which the
//! validator rejects.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};
use cs_assets::rof::mount_rof_into;
use cs_assets::vfs::{INSTALL_NAMESPACE, MountBuilder, SessionBuilder};
use cs_content::catalog::baseline::{SCRAPBOOK_CONTAINER, SCRAPBOOK_MEMBER, retail_baseline};
use cs_content::config::{ConfigDocument, RecordSchema};
use cs_content::scrapbook::{DiscoveredScrapbook, MEMENTO_IMAGE_PREFIX, ScrapbookCatalog, audit};
use cs_types::asset_id::{
    AssetKey, MountId, MountNamespace, PrecedenceClass, ResolveContext, SourceSpan,
};
use cs_types::evidence::ContentHash;

/// How many pages the owner's installation spells, and how many records they
/// hold: the measured shape the discovery, the independent derivation and the
/// acceptance test all have to agree on.
const EXPECTED_RECORDS: usize = 461;
const EXPECTED_PAGES: usize = 25;
const EXPECTED_WITH_ARTWORK: usize = 294;
const EXPECTED_MEMENTO_IMAGES: usize = 15;
const EXPECTED_MEMBER_SHA256: &str =
    "28b5144c54120f52c36717a3f1e094cb75845ecb1f854334a5686d5f6c6af5c1";

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_f47_d_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    // The candidate tree must be the tree that was actually tested: a stale
    // report from another commit is exactly what this check refuses.
    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    // The acceptance suite is the evidence: parse its recorded output.
    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_f47_d_` tests were recorded in {}",
        log_path.display()
    );
    assert!(
        suite.failed == 0,
        "{} `accept_f47_d_` tests failed in the recorded run",
        suite.failed
    );

    // Capability coverage is checked, never assumed: the report declares
    // `retail` and `gpu` only because both kinds of test are in this log.
    for required in ["accept_f47_d_retail_", "accept_f47_d_retail_gpu_"] {
        let line = suite
            .assertions
            .iter()
            .find(|(name, _)| name.contains(required))
            .unwrap_or_else(|| {
                panic!(
                    "the {required} acceptance test did not run: F47-D requires its capability, \
                     run step 1 with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(line.1, "pass", "{required} must pass; got {}", line.1);
    }
    assert!(
        suite
            .assertions
            .iter()
            .any(|(name, _)| name.contains("accept_f47_d_")
                && !name.contains("accept_f47_d_retail_")),
        "synthetic task tests must be present alongside the retail ones"
    );

    // `source` hashes describe the real installation, measured by the very
    // production code the task's consumer wires in.
    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_hash = fingerprint(&found.manifest);
    let install_sha256 = install_hash.to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The consumer trace: the production discovery and the production audit
    // over the original installation, written into the evidence directory.
    let discovered = DiscoveredScrapbook::discover(&game_dir)
        .expect("the production discovery must read the original scrapbook table");
    assert_eq!(discovered.install_sha256, install_sha256);
    assert_eq!(
        discovered.member_sha256, EXPECTED_MEMBER_SHA256,
        "the member the acceptance run read is the member every other stage fingerprinted"
    );
    assert_eq!(discovered.records, EXPECTED_RECORDS);
    assert_eq!(discovered.page_count(), EXPECTED_PAGES);
    assert_eq!(discovered.items_with_artwork(), EXPECTED_WITH_ARTWORK);

    let discovery_path = evidence_dir.join("scrapbook-discovery.json");
    fs::write(&discovery_path, discovered.json()).expect("the discovery artifact is written");

    let baseline = retail_baseline(&game_dir)
        .expect("the production baseline must read the original installation");
    assert_eq!(
        baseline.install_sha256, install_sha256,
        "the baseline's installation fingerprint is production discovery's"
    );
    let declared = ScrapbookCatalog::new(Vec::new()).expect("an empty declaration validates");
    let report = audit(&discovered, &declared, &baseline.catalog);
    assert_eq!(report.records, EXPECTED_RECORDS);
    assert_eq!(report.pages, EXPECTED_PAGES);
    assert_eq!(report.items_with_artwork, EXPECTED_WITH_ARTWORK);
    assert_eq!(report.memento_named_images, EXPECTED_MEMENTO_IMAGES);
    assert_eq!(
        report.progression_missions, 24,
        "the campaign's missions are the progression the audit compares against"
    );
    assert_eq!(report.declared, 0, "the runtime declares no entry yet");
    assert_eq!(report.undeclared_items, EXPECTED_RECORDS);
    assert_eq!(report.unlock_known, 0);
    assert_eq!(report.replay_links, 0);
    assert_eq!(report.declared_mementos, 0);
    assert!(
        !report.is_complete(),
        "the audit must report the declared catalog as incomplete, not pass"
    );
    let audit_path = evidence_dir.join("scrapbook-audit.json");
    fs::write(&audit_path, report.json()).expect("the audit artifact is written");

    // The independent derivation: a second mount of the same archive, a
    // second parse of the table, its own grouping and its own picture join.
    // It shares only the byte readers with the discovery above.
    let (member, spellings) = read_member_and_index(&game_dir, install_hash);
    assert_eq!(
        sha256(&member).to_hex(),
        EXPECTED_MEMBER_SHA256,
        "the second reading fingerprints the same decoded member"
    );
    let second = Independent::read(&member, &spellings);
    assert_eq!(second.records, discovered.records, "one row per record");
    assert_eq!(second.pages, discovered.page_count(), "the same page runs");
    assert_eq!(
        second.with_artwork,
        discovered.items_with_artwork(),
        "the same pictures found"
    );
    assert_eq!(
        second.without_artwork,
        EXPECTED_RECORDS - EXPECTED_WITH_ARTWORK,
        "and the same pictures missing"
    );
    assert_eq!(second.memento_images, EXPECTED_MEMENTO_IMAGES);

    // The GPU half: one capture per discovered page, written by the retail
    // capture test on the real adapter and hashed here.
    let mut captures = Vec::new();
    for page in 0..EXPECTED_PAGES {
        let prefix = format!("f47-d-page-{page:03}-");
        let found = fs::read_dir(&evidence_dir)
            .unwrap_or_else(|error| panic!("read {}: {error}", evidence_dir.display()))
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(&prefix) && name.ends_with(".png"))
            })
            .collect::<Vec<_>>();
        assert_eq!(
            found.len(),
            1,
            "page {page} has exactly one capture in {}",
            evidence_dir.display()
        );
        captures.push(found.into_iter().next().expect("one capture"));
    }
    assert_eq!(
        captures.len(),
        EXPECTED_PAGES,
        "every discovered page drew a measured frame"
    );

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let mut artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&discovery_path, "json", &evidence_dir),
        artifact(&audit_path, "json", &evidence_dir),
    ];
    for capture in &captures {
        artifacts.push(artifact(capture, "capture", &evidence_dir));
    }

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"F47-D\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"gpu\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \
         \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [{}],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        // Deliberately empty (the brackets are the template's): see the
        // module doc — the task's own acceptance is complete, and the
        // product-coverage limits live in `review.method`, the hashed
        // artifacts, `docs/findings/` and the follow-up tasks instead of
        // being deleted.
        "",
        jstr(&review_identity()),
        jstr(&review_method()),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"F47-D\"",
        "\"claim\": \"implemented\"",
        "\"capabilities\": [\"retail\", \"gpu\", \"synthetic\"]",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
        "\"unknowns\": [],",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// The table as a second reader sees it: a parse, a grouping and a picture
/// join that never call `DiscoveredScrapbook::discover`.
struct Independent {
    records: usize,
    pages: usize,
    with_artwork: usize,
    without_artwork: usize,
    memento_images: usize,
}

impl Independent {
    fn read(member: &[u8], spellings: &[String]) -> Self {
        let span = SourceSpan::new(
            ContentHash::from_bytes([0x11; 32]),
            SCRAPBOOK_CONTAINER,
            Some(SCRAPBOOK_MEMBER),
            0,
            member.len() as u64,
            Some(sha256(member)),
        )
        .expect("a valid span over the decoded member");
        let mut context = cs_formats::ParseContext::with_defaults(SCRAPBOOK_CONTAINER);
        let document = ConfigDocument::read(&mut context, span, member)
            .expect("the scrapbook member reads as the keyed-list table");
        let mut stems: BTreeSet<String> = BTreeSet::new();
        for spelling in spellings {
            let Some((_, name)) = spelling.rsplit_once('/') else {
                continue;
            };
            stems.insert(name.split('.').next().unwrap_or(name).to_ascii_lowercase());
        }
        let mut groups: BTreeMap<u32, Vec<String>> = BTreeMap::new();
        let mut images: BTreeSet<String> = BTreeSet::new();
        let mut with_artwork = 0;
        for entry in document.entries() {
            if RecordSchema::for_entry(entry) != Some(RecordSchema::Scrapbook) {
                continue;
            }
            let Ok(key) = String::from_utf8(entry.key.clone()) else {
                continue;
            };
            let Some((first, ..)) = key.split_once('_') else {
                continue;
            };
            let Ok(page) = first.parse::<u32>() else {
                continue;
            };
            groups.entry(page).or_default().push(key);
            let image = image_field(entry).to_ascii_lowercase();
            if image.starts_with(MEMENTO_IMAGE_PREFIX) {
                images.insert(image.clone());
            }
            if stems.contains(&image) {
                with_artwork += 1;
            }
        }
        let records: usize = groups.values().map(Vec::len).sum();
        Self {
            records,
            pages: groups.len(),
            with_artwork,
            without_artwork: EXPECTED_RECORDS - with_artwork,
            memento_images: images.len(),
        }
    }
}

/// The `ImageName` field (position 2) of one record, as written.
fn image_field(entry: &cs_content::config::ConfigEntry) -> String {
    match &entry.value {
        cs_content::config::RawValue::Fields(fields) => fields
            .get(2)
            .map(|field| String::from_utf8_lossy(field.value()).into_owned())
            .unwrap_or_default(),
        cs_content::config::RawValue::Unsplit { .. } => String::new(),
    }
}

/// One reading of the archive that yields both the decoded table member and
/// every member spelling the container holds.
fn read_member_and_index(root: &Path, install: ContentHash) -> (Vec<u8>, Vec<String>) {
    let id: String = format!("rof-{}", SCRAPBOOK_CONTAINER.to_ascii_lowercase())
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '.' {
                character
            } else {
                '-'
            }
        })
        .collect();
    let mut builder = SessionBuilder::new(ResolveContext::new(install));
    let source = mount_rof_into(
        &mut builder,
        MountBuilder::new(
            MountId::new(&id).expect("a valid mount id"),
            MountNamespace::new(INSTALL_NAMESPACE).expect("a valid namespace"),
            PrecedenceClass::Shared,
            SCRAPBOOK_CONTAINER,
        )
        .retail(),
        &root.join(SCRAPBOOK_CONTAINER),
    )
    .expect("the scrapbook archive mounts");
    let key = AssetKey::from_spelling(INSTALL_NAMESPACE, SCRAPBOOK_MEMBER, "default")
        .expect("a valid asset key");
    let read = source
        .read(&key)
        .unwrap_or_else(|error| panic!("{SCRAPBOOK_MEMBER}: the member must decode: {error:?}"));
    let spellings = source
        .members()
        .map(|member| member.spelling.clone())
        .collect();
    (read.data, spellings)
}

/// The `review.identity` this harness records.
///
/// The literal below is the text this repository's committed
/// `docs/findings/evidence/F47-D.json` carries, and it is the first string in
/// this function on purpose: `tools/tests/test_evidence_review_identity.py`
/// reads a harness's recorded identity out of exactly this shape
/// (`fn review_identity() -> String`, a literal) and cross-checks it against
/// the committed report, so the text here and the report cannot drift apart
/// silently.
///
/// A reviewing agent supplies their own text through `CS_EVIDENCE_REVIEW` and
/// replaces the literal with it in the same commit, which is how the report
/// then names the review that actually happened.
fn review_identity() -> String {
    let recorded = String::from(
        "implementer: bunny-2/bunny-2 (opencode/mimo-v2.6-Flash, Rally #201 implement claim of \
         2026-10-08T10:06:07Z, submitted at 090fe720); reviewer: bunny-2/bunny-2 (Rally #201 \
         review claim of 2026-10-08T11:51:33Z): a separate session whose context was fresh and \
         never saw the implementation, but the same agent name and the same model as the \
         implementer, so per AGENTS.md this review is NOT independent evidence for format, \
         mission-semantics and fidelity claims, and no agent review replaces the owner's human \
         approval. The reviewer re-read the production discovery and the production audit, ran \
         all four checks on the rebased branch, fixed two defects himself (the mangled Document \
         refusal text and a missing contiguity assertion over the 25 retail page runs) and \
         regenerated this report on the tree of the commit those fixes landed in; the only later \
         deltas are this report's own copy under docs/findings/evidence/ and the finding's review \
         section, neither of which the acceptance suite reads. `checked` is the ceiling for an \
         agent review; `retail` here means read access to the owner's installation and never a \
         run of the original executable",
    );
    std::env::var("CS_EVIDENCE_REVIEW").unwrap_or(recorded)
}

/// The `review.method` this harness records: what was run, and the
/// product-completeness limits this report does not claim away.
///
/// The limitations are quoted here rather than deleted from the record (the
/// 2026-09-28 owner directive), so a reader of the committed report sees them
/// next to the passing assertions.
fn review_method() -> String {
    let recorded = String::from(
        "acceptance suite run locally with the retail and gpu capabilities; this harness derives \
         every field from the recorded log, production discovery of $CS_GAME_DIR, the production \
         scrapbook discovery and audit over that installation, the production baseline report \
         (the consumer trace), the page captures the retail gpu test wrote through \
         cs_app::ui::front_end::capture_artwork on the real adapter, an independent second \
         reading of the table member, rustc and Cargo.lock; validated with \
         tools/validate_evidence.py --require-pass. The consumer trace is \
         cs_content::scrapbook::DiscoveredScrapbook::discover + audit, which read the \
         GOSDATA/ASSETS/crimson.rof mount through cs_assets::rof::mount_rof_into and the \
         ASSETS/SCRAPBOOK.CSV member through cs_content::config::ConfigDocument, grouping records \
         by their own entry keys (never by line number) and joining each item's picture to the \
         container's own member spellings. The independent derivation shares only the byte \
         readers: it mounts the archive again, re-parses the member, re-groups the keys and \
         re-joins the pictures without calling the discovery. Capability coverage is checked, not \
         assumed: the retail and retail_gpu acceptance tests must be present and pass in the \
         recorded log, and the captures are hashed here. Implementer mutation probes on this \
         branch: removing the artwork join fails the retail test on the measured 294; keying a \
         record by line number instead of its entry key fails the identity agreement with \
         F14-D.8's catalog rows; refusing a member with no scrapbook record instead of returning \
         an empty reading is what the error test pins; and dropping the audit's unmatched-entry \
         arm fails the synthetic audit test on declared_without_original.",
    );
    recorded
        + &UNKNOWN_LIMITATIONS
            .iter()
            .map(|limitation| format!(" LIMITATION: {limitation}"))
            .collect::<String>()
}

/// The limitations this stage records instead of guessing, each naming the
/// affected content and the task that resolves it (AGENTS owner directive,
/// 2026-09-28: a limitation must survive into machine-readable evidence).
const UNKNOWN_LIMITATIONS: &[&str] = &[
    "The runtime declares **none** of the 461 original items: there is no production \
     `ScrapbookCatalog` anywhere in the tree (F47-A's entries are authored fixtures, F47-B \
     persists whatever it is handed and F47-C projects whatever it is given), so the audit reports \
     every original item as `undeclared_items` and `is_complete()` is false. Affected content: no \
     scrapbook page, memento or replay link is reachable by ordinary play, and AC04 can only be \
     re-run as complete once a declared catalog exists. Resolving tasks: F47-C1 (#760, dispatch \
     the front-end scrapbook screen) plus a declared-catalog task built from the original table \
     without inventing unlock rules; the follow-ups filed from #201 name both.",
    "The original table documents **no unlock field**: its sixteen documented positions are the \
     layout fields F12-I typed, `Objective` is the only one whose name could carry progression, \
     and its meaning is unmeasured (values -12..31 over 26 distinct values, converted but never \
     read as a rule). Every `Known` rule of a declared entry is therefore reported as \
     `unlock_unbacked`. Affected content: every unlock path, hidden page and reward claim in F47 \
     and F42. Resolving tasks: the native-behaviour work (F38) and the mission-language stages \
     (F13/F07-D), which own decoding what the original engine evaluates.",
    "The original table documents **no mission field**, so a replay link's target cannot be read \
     from it: the original's table-of-contents script does name a replay control, which proves a \
     replay exists but not what it launches. Affected content: every replay link's mission and \
     variant in F47-C's launch. Resolving tasks: F13/F38 for the control's callback and the \
     campaign bindings for the mission identity.",
    "The table holds **no memento record**: every schema-covered record is a `Mission_Spread_Item`, \
     and the original memento-selection script receives its picture name from a native callback \
     this engine has not decoded, so neither the memento set nor its order nor its unlock state \
     can be claimed. Affected content: the cabin memento choice (spec F47 non-negotiable 5) and \
     the `EntryKind::Memento` entries of any declared catalog. Resolving tasks: F38 (native \
     behaviour bindings) and F45's cabin screen work.",
    "167 of the 461 items name a picture (`Snap_*`) that exists nowhere in the installation's \
     containers, so they are counted under the `item_without_artwork` gap and can never draw a \
     frame; every one of the 25 pages still has at least one picture the container holds. Affected \
     content: those 167 items' artwork. Resolving tasks: F47's declared catalog (which must carry \
     the missing-picture state rather than a placeholder) and, if the owner wants them, a capture \
     stage for runtime snapshots.",
    "A page is this stage's reading of a measured structure (the entry key's first component: 25 \
     contiguous runs, agreeing with the member's own section comments and the sibling \
     `SB_<page>_<spread>_<name>` picture naming); the member documents no key grammar, so whether \
     the original calls the component a page is unmeasured. Affected content: the page grouping \
     and the page numbering of every scrapbook screen. Resolving task: the F13/F38 work above.",
    "The 25 captures are this engine's renderer drawing decoded original pixels on the real \
     adapter; no original executable ran in any agent session, so nothing here compares with what \
     the original presents (its layout, scaling or compositing). Affected content: every visual \
     fidelity claim for F47. Resolving task: REF-OWNER-FIRST-CAPTURE / #358, an owner-supplied \
     original run.",
];

// ------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_content/tests/evidence_report_f47_d.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/F47-D` written relative to the
/// workspace root in the module doc must be re-anchored here.
fn workspace_path(as_described: &str) -> PathBuf {
    let path = PathBuf::from(as_described);
    if path.is_absolute() {
        return path;
    }
    workspace_root().join(path)
}

/// The workspace root, located through git rather than through the package
/// layout, so the harness cannot read a neighbouring checkout by accident.
fn workspace_root() -> PathBuf {
    PathBuf::from(git(&["rev-parse", "--show-toplevel"]))
}

fn git(args: &[&str]) -> String {
    let output = Command::new("git").args(args).output().expect("git runs");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn rustc_version() -> String {
    let output = Command::new("rustc")
        .arg("--version")
        .output()
        .expect("rustc runs");
    assert!(output.status.success(), "rustc --version failed");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// The locked version of one `Cargo.lock` package: read, never asserted from
/// memory.
fn locked_version(package: &str) -> String {
    let lock_path = workspace_root().join("Cargo.lock");
    let lock = fs::read_to_string(&lock_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", lock_path.display()));
    let mut wanted = false;
    for line in lock.lines() {
        let line = line.trim();
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

// ---------------------------------------------------------- log parsing ---

/// What the recorded `cargo test` output says actually happened.
#[derive(Debug, Default)]
struct Suite {
    discovered: u64,
    executed: u64,
    passed: u64,
    failed: u64,
    ignored: u64,
    /// `(test name, "pass" | "fail" | "unknown")`, in log order, deduplicated.
    assertions: Vec<(String, &'static str)>,
}

/// Extracts the libtest summaries and the per-test results of the
/// `accept_f47_d_` tests from a recorded `cargo test` output.
fn parse_suite(log: &str) -> Suite {
    let mut suite = Suite::default();
    let mut pending: VecDeque<String> = VecDeque::new();
    for line in log.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("test result:") {
            for (count, kind) in summary_fields(trimmed) {
                match kind {
                    "passed" => suite.passed += count,
                    "failed" => suite.failed += count,
                    "ignored" => suite.ignored += count,
                    _ => {}
                }
            }
            continue;
        }
        if pending.front().is_some() {
            if trimmed == "ok" {
                let name = pending.pop_front().expect("pending test");
                record(&mut suite, name, "pass");
                continue;
            }
            if trimmed == "FAILED" {
                let name = pending.pop_front().expect("pending test");
                record(&mut suite, name, "fail");
                continue;
            }
        }
        let mut cursor = trimmed;
        while let Some(position) = cursor.find("test ") {
            let after = &cursor[position + 5..];
            let Some(separator) = after.find(" ... ") else {
                break;
            };
            let name = after[..separator].to_owned();
            let tail = &after[separator + 5..];
            cursor = tail;
            if !name.contains("accept_f47_d_") {
                continue;
            }
            match tail.split_whitespace().next() {
                Some("ok") => record(&mut suite, name, "pass"),
                Some("FAILED") => record(&mut suite, name, "fail"),
                _ => pending.push_back(name),
            }
        }
    }
    suite.assertions.dedup_by(|left, right| left.0 == right.0);
    suite.executed = suite.passed + suite.failed;
    suite.discovered = suite.passed + suite.failed + suite.ignored;
    suite
}

/// `(count, kind)` pairs of one `test result:` summary line.
fn summary_fields(line: &str) -> Vec<(u64, &str)> {
    let mut fields = Vec::new();
    for segment in line["test result:".len()..].split(';') {
        let words: Vec<&str> = segment.split_whitespace().collect();
        for pair in words.windows(2) {
            if let Ok(count) = pair[0].parse::<u64>()
                && matches!(pair[1], "passed" | "failed" | "ignored")
            {
                fields.push((count, pair[1]));
                break;
            }
        }
    }
    fields
}

fn record(suite: &mut Suite, name: String, status: &'static str) {
    if suite.assertions.iter().any(|(seen, _)| *seen == name) {
        return;
    }
    suite.assertions.push((name, status));
}

// ------------------------------------------------------------ artifacts ---

/// One referenced artifact: hashed here with the production SHA-256 the sibling
/// crate implements (the validator re-hashes it with `hashlib` independently).
fn artifact(source: &Path, kind: &str, evidence_dir: &Path) -> (String, String, String) {
    let name = source
        .file_name()
        .expect("artifact has a file name")
        .to_string_lossy()
        .into_owned();
    let target = evidence_dir.join(&name);
    if source != target {
        fs::copy(source, &target).unwrap_or_else(|error| {
            panic!("copy {} -> {}: {error}", source.display(), target.display())
        });
    }
    let bytes =
        fs::read(&target).unwrap_or_else(|error| panic!("read {}: {error}", target.display()));
    (name, sha256(&bytes).to_hex(), kind.to_owned())
}

// ------------------------------------------------------------ rendering ---

struct Engine {
    rust: String,
    bevy: String,
    avian: String,
}

fn engine_json(engine: &Engine) -> String {
    format!(
        "{{\"rust\": {}, \"bevy\": {}, \"avian\": {}}}",
        jstr(&engine.rust),
        jstr(&engine.bevy),
        jstr(&engine.avian)
    )
}

fn assertion_array(assertions: &[(String, &'static str)]) -> String {
    let items: Vec<String> = assertions
        .iter()
        .map(|(name, status)| {
            format!(
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\"]}}",
                jstr(name)
            )
        })
        .collect();
    items.join(", ")
}

fn artifact_array(artifacts: &[(String, String, String)]) -> String {
    let items: Vec<String> = artifacts
        .iter()
        .map(|(name, digest, kind)| {
            format!(
                "{{\"path\": {}, \"sha256\": {digest:?}, \"kind\": {kind:?}}}",
                jstr(name)
            )
        })
        .collect();
    items.join(", ")
}

fn str_array(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|item| jstr(item)).collect();
    format!("[{}]", quoted.join(", "))
}

/// A JSON string literal: quoted and escaped, so no report field can break out
/// of its string.
fn jstr(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if (control as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// RFC 3339 with whole seconds and `Z`, which `datetime.fromisoformat`
/// accepts after the validator's `Z` → `+00:00` replacement.
fn iso_utc_now() -> String {
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock is after 1970")
        .as_secs() as i64;
    let (year, month, day, hour, minute, second) = civil_from_unix(epoch);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to a UTC calendar
/// date, because `std` has no date formatting.
fn civil_from_unix(seconds: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year_of_day = era * 400 + year_of_era;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = (if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    }) as u32;
    let year = if month <= 2 {
        year_of_day + 1
    } else {
        year_of_day
    };
    (
        year,
        month,
        day,
        (rest / 3_600) as u32,
        ((rest % 3_600) / 60) as u32,
        (rest % 60) as u32,
    )
}
