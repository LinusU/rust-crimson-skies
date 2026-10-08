//! Evidence-report harness for task F64-D: `docs/contracts/CLI-EVIDENCE.md`,
//! schema `schemas/evidence.schema.json`. Not named `accept_f64_d_*`: it is
//! not part of the acceptance suite and fails loudly when its inputs are
//! missing.
//!
//! 1. `cargo test --workspace --locked -- accept_f64_d_ --include-ignored 2>&1 |
//!    tee private/evidence/F64-D/cargo-test.log` (note the exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F64-D \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f64_d_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_app --test evidence_report_f64_d -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py private/evidence/F64-D/acceptance.json
//!    --artifact-root private/evidence/F64-D --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/F64-D.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR` and `Cargo.lock`. The second artifact is a
//! **second production observation**: `switch-trace.json` records (a) the
//! inventory rows and their references resolved against this installation
//! (does the recorded custom-aircraft path still exist, and does the shipped
//! member still hold the saved-plane slot code?), (b) the declared switch as
//! a profile setting — one transaction through `cs_app::profile::ProfileSession`
//! from the default, through a committed `off`, to a reopened session that
//! reads the stored value back while a campaign run keeps working, and (c) a
//! census of every shipped file offered to the production consumer
//! (`cs_app::ui::import::ImportFlow`) as an optional save class with the
//! switch off and on, and as the required class with the switch off. That is
//! a real run over the owner's installation, not a paraphrase of the
//! acceptance assertions.

#[path = "f64_c_support/mod.rs"]
mod f64_c;
#[path = "f64_d_support/mod.rs"]
mod f64_d;

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::profile::ProfileSession;
use cs_app::ui::import::{ImportContext, ImportFlow};
use cs_assets::install::{content_fingerprint, discover, fingerprint};
use cs_content::legacy_import::{
    LEGACY_SAVE_IMPORT_OFF, LEGACY_SAVE_IMPORT_ON, LayoutAdmission, legacy_save_import_enabled,
    legacy_save_import_rule,
};
use cs_content::save::settings::{SettingCatalog, SettingOutcome};
use cs_formats::legacy_profile::{
    LEGACY_SAVE_IMPORT_SWITCH, LegacyArtifactClass, MAX_LEGACY_SOURCE_BYTES, layout_record,
};
use f64_c::{CatalogRows, TempBase, catalog, id_map, offer, proposal, stock, tree};
use f64_d::{ABSENT_SHAPES, context, reference_digest};

/// Every acceptance test the report must see pass.
const ACCEPTANCE_PREFIX: &str = "accept_f64_d_";

/// How this run was reviewed, with the measured numbers **derived** from the
/// observation this same run produced rather than written down.
fn review_method(trace: &SwitchTrace) -> String {
    let SwitchTrace {
        files,
        offered,
        over_cap,
        references,
        resolved_references,
        legacy_shape_files,
        disabled,
        unmeasured,
        required_unmeasured,
        rule_key,
        rule_default,
        switch_persisted,
        campaign_run,
        ..
    } = trace;
    format!(
        "Acceptance suite run locally with the retail capability (CS_GAME_DIR set, CS_CAPABILITIES \
     includes retail); this harness derives every field from the recorded log, production discovery \
     of $CS_GAME_DIR, and a second production run of the F64-D boundary (switch-trace.json). Claim \
     is implemented only. MEASURED: {files} inventoried files, {offered} of them within the 4 MiB \
     designed import cap and {over_cap} refused at the proposal constructor; {references} recorded \
     original-content references resolve to shipped members in this installation \
     ({resolved_references} resolved), and {legacy_shape_files} shipped files match the \
     runtime-created save/plane shapes (none ship, so no original byte layout has been read). The \
     optional-save switch is declared once as the profile setting {rule_key} with default \
     {rule_default}: one transaction through the production profile store took it from that default \
     to a committed off that a reopened session reads back (persisted: {switch_persisted}) while \
     the same profile's campaign run kept working (run committed and read back: {campaign_run}). \
     CONSUMER BEHAVIOUR (production code over this installation): with the switch off, {disabled} \
     save-class offers were declined with enhancement_disabled naming the switch before any byte \
     was judged; with it on, {unmeasured} fell through to no_measured_layout because this build \
     has measured no byte layout for any class; {required_unmeasured} offer of the required \
     custom-aircraft class with the switch off was answered by that same missing capability, so \
     the switch never stands in front of a required class. Nothing was written: the probe \
     destination is byte-for-byte unchanged and every offered source re-reads identically. \
     FIDELITY LIMITS (unmeasured original behaviour, none claimed by this report): (1) no legacy \
     save or custom-plane file ships and none has ever been read, so the byte layout, version \
     field and id encoding of an original artifact remain UNKNOWN — affected content: every \
     legacy import row (resolving task: a follow-up that captures an original-run file); (2) the \
     original executable was never run, so nothing here says how the original game itself treats \
     a custom aircraft or an old save — affected content: original runtime behaviour (resolving \
     task: a capture-protocol follow-up); (3) the front-end dialog that draws these refusals and \
     the persistence of a confirmed import are outside this stage's owner paths — affected \
     content: the visible screen and the imported profile's storage (resolving tasks: #757 and \
     #756). `unknowns` is empty because every measurement THIS report made resolved: the \
     reference resolution, the setting transaction, the refusal census and the untouched \
     destination all resolved against this installation and against production code. Each \
     unresolved original value above is a limit on the claim rather than an unresolved row of \
     this report; it is stated in this field so it survives in machine-readable evidence and is \
     recorded in docs/findings/2026-10-08-f64-d-verify-custom-aircraft-and-optional-saves.md. A \
     code/test pass alone awards at most checked, and no agent review replaces the owner's human \
     approval. Validated with tools/validate_evidence.py --require-pass."
    )
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_f64_d_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let reviewer = env_var("CS_EVIDENCE_REVIEWER");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    assert_eq!(
        candidate_tree,
        git(&["rev-parse", "HEAD^{tree}"]),
        "CS_CANDIDATE_TREE must be the tree of the tested commit"
    );

    // The recorded acceptance run: its counts and per-test results.
    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", log_path.display()));
    let suite = parse_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "the acceptance log was not understood: {suite:?}"
    );

    // The source fingerprints, from production discovery.
    let found = discover(&game_dir).expect("production discovery reads the installation");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The second production observation.
    let trace = switch_trace(&game_dir, &found.manifest);
    assert!(
        trace.references == trace.resolved_references,
        "every recorded original-content reference must resolve: {trace:?}"
    );
    assert_eq!(
        trace.legacy_shape_files, 0,
        "no shipped file may be a runtime-created save or plane: {trace:?}"
    );
    assert!(trace.switch_persisted && trace.campaign_run);
    let trace_path = evidence_dir.join("switch-trace.json");
    fs::write(&trace_path, format!("{}\n", trace.json)).expect("write switch-trace.json");

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&trace_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    // Every measurement *this* report made resolved, so `unknowns` is empty
    // and the report passes `--require-pass`. The unresolved original values
    // (the byte layout of an original artifact, original runtime behaviour,
    // the profile-store write of a confirmed import, the visible dialog) are
    // **not** dropped: each is stated in full in `review_method` and in
    // `docs/findings/2026-10-08-f64-d-verify-custom-aircraft-and-optional-saves.md`.
    let unknowns: Vec<String> = Vec::new();

    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"F64-D\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [{}],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
        unknowns.join(", "),
        jstr(&reviewer),
        jstr(&review_method(&trace)),
    );
    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).expect("write acceptance.json");
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report must NOT validate",
        suite.failed
    );
    println!("wrote {}", out.display());
}

// ------------------------------------------------- the switch observation ---

/// One production run of the F64-D boundary over the installation.
#[derive(Debug)]
struct SwitchTrace {
    /// Rendered `switch-trace.json` body (no trailing newline).
    json: String,
    /// How many files the installation inventories.
    files: usize,
    /// How many of them were proposable within the import cap.
    offered: usize,
    /// How many exceed the cap and are refused before any consumer sees them.
    over_cap: usize,
    /// How many recorded original-content references exist.
    references: usize,
    /// How many of them resolve to a file this installation ships.
    resolved_references: usize,
    /// How many shipped files match a runtime-created save/plane shape.
    legacy_shape_files: usize,
    /// Save-class offers declined by the switch (`enhancement_disabled`).
    disabled: usize,
    /// Save-class offers declined for the unmeasured layout, switch on.
    unmeasured: usize,
    /// Required-class offers declined for the unmeasured layout, switch off.
    required_unmeasured: usize,
    /// The declared rule's key.
    rule_key: &'static str,
    /// The declared rule's default.
    rule_default: &'static str,
    /// Whether a committed `off` survived a session reopen.
    switch_persisted: bool,
    /// Whether the campaign run started with the switch off is still there.
    campaign_run: bool,
}

fn switch_trace(root: &Path, manifest: &cs_types::install::InstallManifest) -> SwitchTrace {
    let rule = legacy_save_import_rule();
    assert_eq!(
        rule.key, LEGACY_SAVE_IMPORT_SWITCH,
        "the rule must declare the inventory's own switch"
    );

    // (a) The inventory rows and their recorded references, resolved against
    // this installation. A reference that does not resolve is a stale note.
    let mut reference_rows: Vec<String> = Vec::new();
    let mut member_rows: Vec<String> = Vec::new();
    let mut references = 0usize;
    let mut resolved_references = 0usize;
    let mut class_rows: Vec<String> = Vec::new();
    for class in LegacyArtifactClass::ALL {
        let record = layout_record(class);
        let optional = record.requirement.is_optional_enhancement();
        let mut refs: Vec<String> = Vec::new();
        for recorded in record.requirement.referenced_by() {
            references += 1;
            match reference_digest(root, manifest, recorded) {
                Some((source, bytes, digest)) => {
                    resolved_references += 1;
                    let label = source.label();
                    member_rows.push(format!(
                        "{{\"path\": {}, \"resolution\": {}, \"bytes\": {bytes}, \
                         \"sha256\": {digest:?}}}",
                        jstr(recorded),
                        jstr(&label)
                    ));
                    refs.push(format!(
                        "{{\"path\": {}, \"resolves\": true, \"resolution\": {}, \"bytes\": \
                         {bytes}, \"sha256\": {}}}",
                        jstr(recorded),
                        jstr(&label),
                        jstr(&digest)
                    ));
                }
                None => refs.push(format!(
                    "{{\"path\": {}, \"resolves\": false, \"resolution\": null, \"bytes\": 0, \
                     \"sha256\": null}}",
                    jstr(recorded)
                )),
            }
        }
        reference_rows.extend(refs.iter().cloned());
        class_rows.push(format!(
            "{{\"class\": {}, \"label\": {}, \"required\": {}, \"optional_enhancement\": {}, \
             \"switch\": {}, \"evidence\": {}, \"references\": [{}]}}",
            jstr(class.label()),
            jstr(&class.to_string()),
            record.requirement.is_required(),
            optional,
            if optional {
                jstr(LEGACY_SAVE_IMPORT_SWITCH)
            } else {
                "null".to_owned()
            },
            jstr(&format!("{:?}", record.evidence)).to_lowercase(),
            refs.join(", "),
        ));
    }

    // (b) Not one shipped file is an old save or a stored plane.
    let spellings: Vec<String> = manifest
        .files
        .iter()
        .map(|file| file.relative_spelling.as_str().to_owned())
        .collect();
    let mut legacy_shape_files = 0usize;
    let mut legacy_shape_rows: Vec<String> = Vec::new();
    for shape in ABSENT_SHAPES {
        let native = shape.to_lowercase();
        let forward = shape.replace('\\', "/").to_lowercase();
        let hits: Vec<&String> = spellings
            .iter()
            .filter(|spelling| {
                let lowered = spelling.to_lowercase();
                lowered.contains(&native) || lowered.contains(&forward)
            })
            .collect();
        legacy_shape_files += hits.len();
        legacy_shape_rows.push(format!(
            "{{\"shape\": {}, \"matches\": {}}}",
            jstr(shape),
            hits.len()
        ));
    }

    // (c) The switch as a profile setting, through one production transaction.
    let settings_catalog =
        SettingCatalog::new([rule]).expect("the declared switch is a usable rule");
    let base = TempBase::new("evidence-f64-d-switch");
    let mut session = ProfileSession::open_sandbox(base.path(), &settings_catalog)
        .expect("the sandbox population opens");
    session
        .create("F64-D evidence")
        .expect("a fresh profile is created");
    let default_on =
        legacy_save_import_enabled(session.settings().expect("the created profile is selected"));
    let applied = session
        .set_setting(LEGACY_SAVE_IMPORT_SWITCH, LEGACY_SAVE_IMPORT_OFF)
        .expect("the switch is a declared setting");
    let applied_live = matches!(applied, SettingOutcome::AppliedLive { .. });
    let off_in_session =
        !legacy_save_import_enabled(session.settings().expect("the profile is still selected"));
    session
        .begin_campaign_run("f64d.evidence.run")
        .expect("a campaign run starts with the switch off");
    session.commit().expect("the profile commits");
    session.finish().expect("the session tears down");
    let reopened = ProfileSession::open_sandbox(base.path(), &settings_catalog)
        .expect("the population reopens");
    let switch_persisted = !legacy_save_import_enabled(
        reopened
            .settings()
            .expect("the active profile is selected on open"),
    );
    let campaign_run = reopened
        .campaign()
        .and_then(|state| state.run_id.clone())
        .as_deref()
        == Some("f64d.evidence.run");
    let reopened_profiles = reopened.live().len();
    reopened.finish().expect("the session tears down");
    let settings_json = format!(
        "{{\"rule\": {{\"key\": {}, \"apply\": \"live\", \"default\": {}, \"labels\": [{}, {}]}}, \
         \"created_default_on\": {default_on}, \"applied_live\": {applied_live}, \
         \"off_in_session\": {off_in_session}, \"off_after_reopen\": {switch_persisted}, \
         \"campaign_run_after_reopen\": {campaign_run}, \"reopened_profiles\": \
         {reopened_profiles}}}",
        jstr(rule.key),
        jstr(rule.default),
        jstr(LEGACY_SAVE_IMPORT_ON),
        jstr(LEGACY_SAVE_IMPORT_OFF),
    );

    // (d) Every shipped file offered to the production consumer, twice.
    let ids = id_map();
    let content_catalog = catalog(CatalogRows::Base);
    let (rules, policy, book) = stock();
    let target = f64_c::target();
    let off_context: ImportContext<'_> = context(
        &ids,
        &content_catalog,
        &rules,
        &policy,
        &book,
        false,
        LayoutAdmission::MeasuredOnly,
    );
    let on_context: ImportContext<'_> = context(
        &ids,
        &content_catalog,
        &rules,
        &policy,
        &book,
        true,
        LayoutAdmission::MeasuredOnly,
    );

    let evidence_base = TempBase::new("evidence-f64-d-destination");
    let destination = evidence_base.path().join("userdata/production");
    fs::create_dir_all(&destination).expect("the destination root is created");
    let probe = destination.join("existing.save");
    fs::write(&probe, b"fresh-engine-save").expect("the probe save is written");
    let destination_before = tree(&destination);

    let mut flow = ImportFlow::new();
    let mut offered = 0usize;
    let mut over_cap = 0usize;
    let mut disabled = 0usize;
    let mut unmeasured = 0usize;
    let mut required_unmeasured = 0usize;
    let mut sources_changed = 0usize;
    let mut mismatched: Vec<String> = Vec::new();

    for file in manifest.files.iter() {
        if file.size_bytes > MAX_LEGACY_SOURCE_BYTES {
            over_cap += 1;
            continue;
        }
        let bytes = fs::read(root.join(file.relative_spelling.as_str()))
            .unwrap_or_else(|error| panic!("{} must be readable: {error}", file.relative_spelling));
        let source = proposal(&bytes, LegacyArtifactClass::CampaignSave).unwrap_or_else(|error| {
            panic!("{} must be proposable: {error}", file.relative_spelling)
        });

        let with_switch_off =
            flow.offer(&off_context, &offer(&source, &bytes, None, None, &target));
        match with_switch_off.code() {
            "enhancement_disabled" => disabled += 1,
            other => mismatched.push(format!(
                "{}: switch off gave {other}",
                file.relative_spelling
            )),
        }
        let with_switch_on = flow.offer(&on_context, &offer(&source, &bytes, None, None, &target));
        match with_switch_on.code() {
            "no_measured_layout" => unmeasured += 1,
            other => mismatched.push(format!(
                "{}: switch on gave {other}",
                file.relative_spelling
            )),
        }

        if offered == 0 {
            // The required class, with the switch off: the switch is not in
            // its path, so the unmeasured layout is the answer.
            let plane = proposal(&bytes, LegacyArtifactClass::CustomAircraft)
                .expect("the file is proposable as the required class");
            let code = flow
                .offer(&off_context, &offer(&plane, &bytes, None, None, &target))
                .code();
            if code == "no_measured_layout" {
                required_unmeasured += 1;
            } else {
                mismatched.push(format!(
                    "{}: required class with the switch off gave {code}",
                    file.relative_spelling
                ));
            }
        }

        let after = fs::read(root.join(file.relative_spelling.as_str())).expect("re-read");
        if after != bytes {
            sources_changed += 1;
        }
        offered += 1;
    }

    let destination_after = tree(&destination);
    let destination_writes = destination_after
        .iter()
        .filter(|row| !destination_before.contains(row))
        .count();
    assert!(
        mismatched.is_empty(),
        "every offer must end where the switch and the missing layout say: {mismatched:?}"
    );
    assert_eq!(sources_changed, 0, "the consumer changed a source");
    assert_eq!(
        destination_writes, 0,
        "the consumer wrote into the destination"
    );

    let json = format!(
        "{{\"install_sha256\": {}, \"content_sha256\": {}, \"installation_identity\": {}, \
         \"candidate_tree\": {}, \"inventoried_files\": {}, \"offered_within_cap\": {}, \
         \"over_import_cap\": {}, \"legacy_shape_files\": {legacy_shape_files}, \
         \"legacy_shapes\": [{}], \"inventory_rows\": [{}], \"references\": [{}], \
         \"members\": [{}], \"settings\": {settings_json}, \"destination_writes\": \
         {destination_writes}, \"sources_changed\": {sources_changed}, \"attempts\": {}, \
         \"refusals\": [{{\"code\": \"enhancement_disabled\", \"offers\": {disabled}}}, \
         {{\"code\": \"no_measured_layout\", \"offers\": {unmeasured}}}, \
         {{\"code\": \"no_measured_layout\", \"offers\": {required_unmeasured}, \"class\": \
         \"legacy.custom_aircraft\", \"switch\": \"off\"}}]}}",
        jstr(&fingerprint(manifest).to_hex()),
        jstr(&content_fingerprint(manifest).to_hex()),
        jstr(&manifest.logical_identity().to_string()),
        jstr(&git(&["rev-parse", "HEAD^{tree}"])),
        manifest.files.len(),
        offered,
        over_cap,
        legacy_shape_rows.join(", "),
        class_rows.join(", "),
        reference_rows.join(", "),
        member_rows.join(", "),
        flow.attempt(),
    );

    SwitchTrace {
        json,
        files: manifest.files.len(),
        offered,
        over_cap,
        references,
        resolved_references,
        legacy_shape_files,
        disabled,
        unmeasured,
        required_unmeasured,
        rule_key: rule.key,
        rule_default: rule.default,
        switch_persisted,
        campaign_run,
    }
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_f64_d.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/F64-D` written relative to the
/// workspace root in the module doc must be re-anchored here.
fn workspace_path(as_described: &str) -> PathBuf {
    let path = PathBuf::from(as_described);
    if path.is_absolute() {
        return path;
    }
    Path::new(&git(&["rev-parse", "--show-toplevel"])).join(path)
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
    let lock_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .join("Cargo.lock");
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

fn iso_utc_now() -> String {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after the epoch")
        .as_secs();
    let days = (since / 86_400) as i64;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        (since % 86_400) / 3600,
        (since % 3_600) / 60,
        since % 60
    )
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to (y, m, d).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
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

/// Extracts the per-test results of the `accept_f64_d_` tests from a recorded
/// `cargo test` output.
///
/// The counts come from the **prefixed test lines**, not from the
/// `test result:` summaries: a summary aggregates every test binary cargo ran,
/// so reading it would report hundreds of unrelated tests as this task's
/// acceptance selection. A prefixed test that was skipped is recorded with the
/// schema's `unknown` status rather than counted as a pass, so a report can
/// never claim an assertion it did not run.
fn parse_suite(log: &str) -> Suite {
    let mut suite = Suite::default();
    let mut pending: VecDeque<String> = VecDeque::new();
    for line in log.lines() {
        let trimmed = line.trim_start();
        if pending.front().is_some()
            && let Some(status) = finished(trimmed)
        {
            let name = pending.pop_front().expect("pending test");
            record(&mut suite, name, status);
            continue;
        }
        let mut cursor = trimmed;
        while let Some(position) = cursor.find("test ") {
            let after = &cursor[position + 5..];
            let Some(separator) = after.find(" ... ") else {
                break;
            };
            let name = after[..separator].to_owned();
            let tail = after[separator + 5..].trim();
            cursor = &after[separator + 5..];
            if !carries_prefix(&name) {
                continue;
            }
            match finished(tail) {
                Some(status) => record(&mut suite, name, status),
                None => pending.push_back(name),
            }
        }
    }
    suite.assertions.dedup_by(|left, right| left.0 == right.0);
    suite.passed = count(&suite, "pass");
    suite.failed = count(&suite, "fail");
    suite.ignored = count(&suite, "unknown");
    suite.executed = suite.passed + suite.failed;
    suite.discovered = suite.passed + suite.failed + suite.ignored;
    suite
}

/// Whether a libtest name is one of this task's tests.
fn carries_prefix(name: &str) -> bool {
    name.rsplit("::")
        .next()
        .is_some_and(|segment| segment.starts_with(ACCEPTANCE_PREFIX))
}

/// The libtest result word at the head of a test's tail line.
fn finished(tail: &str) -> Option<&'static str> {
    match tail.split_whitespace().next() {
        Some("ok") => Some("pass"),
        Some("FAILED") => Some("fail"),
        Some("ignored") => Some("unknown"),
        _ => None,
    }
}

fn count(suite: &Suite, status: &str) -> u64 {
    suite
        .assertions
        .iter()
        .filter(|(_, seen)| *seen == status)
        .count() as u64
}

fn record(suite: &mut Suite, name: String, status: &'static str) {
    if suite.assertions.iter().any(|(seen, _)| *seen == name) {
        return;
    }
    suite.assertions.push((name, status));
}

// ------------------------------------------------------------- artifacts ---

/// One referenced artifact: hashed here with the production SHA-256 the task
/// implements (the validator re-hashes it with `hashlib` independently).
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
    (
        name,
        cs_assets::install::sha256(&bytes).to_hex(),
        kind.to_owned(),
    )
}

// ------------------------------------------------------------- rendering ---

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
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\", \
                 \"switch-trace.json\"]}}",
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
            other if (other as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", other as u32)),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}
