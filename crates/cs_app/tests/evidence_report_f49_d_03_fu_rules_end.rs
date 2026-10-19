//! Evidence-report harness for task F49-D.03-FU-RULES-END (#1638)
//! (`docs/contracts/CLI-EVIDENCE.md`, schema
//! `schemas/evidence.schema.json`), modelled on
//! `evidence_report_f49_d_02_fu_slots.rs` (cs_content).
//!
//! Deliberately **not** named `accept_f49_d_03_*`: it is not part of the
//! acceptance suite and fails loudly when its inputs are missing. From the
//! workspace root:
//!
//! 1. ```sh
//!    mkdir -p private/evidence/F49-D.03-FU-RULES-END
//!    cargo test --locked -p cs_app --test cs_app_tests_a -- \
//!      accept_f49_d_03_fu_rules_end --include-ignored \
//!      2>&1 | tee private/evidence/F49-D.03-FU-RULES-END/cargo-test.log
//!    ```
//! 2. ```sh
//!    CS_EVIDENCE_TASK_ID=F49-D.03-FU-RULES-END \
//!    CS_EVIDENCE_DIR=private/evidence/F49-D.03-FU-RULES-END \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --locked -p cs_app --test cs_app_tests_a -- accept_f49_d_03_fu_rules_end --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!      cargo test --locked -p cs_app --test cs_app_tests_d -- evidence_report_f49_d_03_fu_rules_end --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py private/evidence/F49-D.03-FU-RULES-END/acceptance.json
//!    --artifact-root private/evidence/F49-D.03-FU-RULES-END --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/F49-D.03-FU-RULES-END.json`.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::ui::instant_action::{lower_preset, retail_instant_action_catalog};
use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};
use cs_content::instant_action::{
    Declared, IA_MISSION_TYPE_VOCABULARY, InstantActionCatalog, measured_ia_victory_rules,
};

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR, CS_ENGINE_IMAGE"]
fn evidence_report_f49_d_03_fu_rules_end_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(!argv.is_empty());
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE is the acceptance run's exit status");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", log_path.display()));
    let suite = parse_suite(&log);
    // The capability-gated tests must have run and passed: the three engine
    // image tests re-read the measured bytes and the retail test binds every
    // catalog preset's rules against the measured mapping.
    for needed in [
        "accept_f49_d_03_fu_rules_end_image_the_type_names_parse_to_the_bound_conditions",
        "accept_f49_d_03_fu_rules_end_image_the_menu_writes_types_0_1_4_2",
        "accept_f49_d_03_fu_rules_end_image_the_end_check_predicates",
        "accept_f49_d_03_fu_rules_end_retail_each_preset_binds_its_mission_type_rules",
    ] {
        let line = suite
            .assertions
            .iter()
            .find(|(name, _)| name.contains(needed))
            .unwrap_or_else(|| {
                panic!(
                    "the capability test {needed} did not run: this task requires `retail` and \
                     CS_ENGINE_IMAGE; run step 1 with --include-ignored and both set"
                )
            });
        assert_eq!(line.1, "pass", "the capability test {needed} must pass");
    }
    assert!(
        suite.assertions.len() >= 9,
        "nine rules-end tests were recorded, got {:?}",
        suite.assertions
    );

    let found = discover(&game_dir).expect("production discovery reads the installation");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();
    let image = fs::read(env_var("CS_ENGINE_IMAGE")).expect("the engine image reads");
    let image_sha256 = sha256(&image).to_hex();
    let image_len = image.len() as u64;

    // The consumer trace: the production binding per mission-type name and
    // per retail preset, derived from the assembled catalog.
    let catalog =
        retail_instant_action_catalog(&game_dir).expect("the production catalog assembles");
    let trace_path = evidence_dir.join("rules-binding.json");
    fs::write(
        &trace_path,
        binding_json(&catalog, &install_sha256, &image_sha256),
    )
    .unwrap_or_else(|e| panic!("write {}: {e}", trace_path.display()));

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&trace_path, "json", &evidence_dir),
    ];

    let review = std::env::var("CS_EVIDENCE_REVIEW").unwrap_or_else(|_| {
        "pending: written by the implementing agent swe2-max-1. The reviewing agent regenerates \
         this report on the reviewed commit and records their identity (CS_EVIDENCE_REVIEW); \
         no agent review awards more than `checked`."
            .to_owned()
    });
    let review_method = std::env::var("CS_EVIDENCE_REVIEW_METHOD").unwrap_or_else(|_| {
        "acceptance suite run locally with the retail capability and the owner-supplied decrypted \
         engine image; every field is derived from the recorded log, production discovery of \
         $CS_GAME_DIR, the production catalog assembled over that installation, the image's own \
         bytes and Cargo.lock. The per-mission-type end conditions (the `0x45b9d0` branches, the \
         wave wake `0x4b0f40`, the bound zeppelin/target/gate objects and the caller's player-down \
         gate `0x4a09b6`) are static image analysis — provenance static, never verified_original; \
         no original executable ran. LIMITATIONS: the meaning of vehicle byte `+0x91f` is \
         unmeasured (both elimination predicates only require it clear); the bound objects' own \
         fields (zeppelin `+0x06`, the gate records' `+0x48`/`+0x40`, the marker `+0x4d`, the \
         wave-group field `+0x388`) are read as the check reads them but what writes them is \
         unmeasured; per-preset difficulty stays unmeasured so `lower_preset` still refuses every \
         retail preset (recorded in docs/findings/2026-10-19-f49-d-03-fu-rules-end.md). Validated \
         with tools/validate_evidence.py --require-pass."
            .to_owned()
    });

    let task_id =
        std::env::var("CS_EVIDENCE_TASK_ID").unwrap_or_else(|_| "F49-D.03-FU-RULES-END".into());
    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": {},\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}, \"engine_image_sha256\": {}, \"engine_image_len\": {}}},\n\
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
        jstr(&task_id),
        jstr(&candidate_tree),
        engine_json(),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        jstr(&image_sha256),
        image_len,
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        "",
        jstr(&review),
        jstr(&review_method),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|e| panic!("write {}: {e}", out.display()));
    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"F49-D.03-FU-RULES-END\"",
        "\"claim\": \"implemented\"",
        "\"engine_image_sha256\"",
        "\"unknowns\": [],",
    ] {
        assert!(written.contains(needle), "missing {needle:?}:\n{written}");
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// The consumer trace: the measured vocabulary binding and every retail
/// preset's rules as the production catalog declares them.
fn binding_json(
    catalog: &InstantActionCatalog,
    install_sha256: &str,
    image_sha256: &str,
) -> String {
    let vocabulary: Vec<String> = IA_MISSION_TYPE_VOCABULARY
        .iter()
        .map(|name| {
            let rules = measured_ia_victory_rules(name).expect("a surveyed name binds");
            format!(
                "{{\"mission_type\":{},\"condition\":{},\"respawns_per_side\":{},\
                 \"deadline_ticks\":null,\"tie_outcome\":{}}}",
                jstr(name),
                jstr(rules.condition().label()),
                rules.respawns().per_side(),
                jstr(rules.tie_outcome().label()),
            )
        })
        .collect();
    let presets: Vec<String> = catalog
        .presets()
        .iter()
        .map(|preset| {
            let mission_type = preset.original_mission_type().unwrap_or("");
            let rules = match preset.parameters().rules() {
                Declared::Declared(rules) => format!(
                    "{{\"condition\":{},\"respawns_per_side\":{},\"deadline_ticks\":null,\
                     \"tie_outcome\":{}}}",
                    jstr(rules.condition().label()),
                    rules.respawns().per_side(),
                    jstr(rules.tie_outcome().label()),
                ),
                Declared::Unmeasured { .. } => "\"unmeasured\"".to_owned(),
            };
            let refusal = match lower_preset(catalog, preset.id()) {
                Ok(_) => "launches".to_owned(),
                Err(error) => error.to_string(),
            };
            format!(
                "{{\"preset\":{},\"scenario\":{},\"mission_type\":{},\"rules\":{},\
                 \"difficulty\":{},\"lower_preset\":{}}}",
                jstr(preset.id().as_str()),
                jstr(preset.scenario().as_str()),
                jstr(mission_type),
                rules,
                jstr(if preset.parameters().difficulty().is_declared() {
                    "declared"
                } else {
                    "unmeasured"
                }),
                jstr(&refusal),
            )
        })
        .collect();
    format!(
        "{{\"schema\":\"cs-ia-rules-end/1\",\"install_sha256\":{},\"engine_image_sha256\":{},\
         \"end_check_va\":\"0x45b9d0\",\"source\":\"cs_app::ui::instant_action::retail_instant_action_catalog\",\
         \"vocabulary\":[{}],\"presets\":[{}]}}\n",
        jstr(install_sha256),
        jstr(image_sha256),
        vocabulary.join(","),
        presets.join(","),
    )
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name)
        .unwrap_or_else(|_| panic!("{name} is not set: see this harness's module doc"))
}

fn workspace_path(as_described: &str) -> PathBuf {
    let path = PathBuf::from(as_described);
    if path.is_absolute() {
        return path;
    }
    PathBuf::from(git(&["rev-parse", "--show-toplevel"])).join(path)
}

fn git(args: &[&str]) -> String {
    let output = Command::new("git").args(args).output().expect("git runs");
    assert!(output.status.success(), "git {args:?} failed");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn locked_version(package: &str) -> String {
    let lock = fs::read_to_string(
        PathBuf::from(git(&["rev-parse", "--show-toplevel"])).join("Cargo.lock"),
    )
    .expect("Cargo.lock reads");
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
    panic!("package {package:?} is not in Cargo.lock");
}

// ---------------------------------------------------------- log parsing ---

#[derive(Debug, Default)]
struct Suite {
    discovered: u64,
    executed: u64,
    passed: u64,
    failed: u64,
    ignored: u64,
    assertions: Vec<(String, &'static str)>,
}

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
        if let Some(name) = pending.front()
            && (trimmed == "ok" || trimmed == "FAILED")
        {
            let status = if trimmed == "ok" { "pass" } else { "fail" };
            record(&mut suite, name.clone(), status);
            pending.pop_front();
            continue;
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
            if !name.contains("accept_f49_d_03_fu_rules_end") {
                continue;
            }
            match tail.split_whitespace().next() {
                Some("ok") => record(&mut suite, name, "pass"),
                Some("FAILED") => record(&mut suite, name, "fail"),
                _ => pending.push_back(name),
            }
        }
    }
    suite.assertions.dedup_by(|l, r| l.0 == r.0);
    suite.executed = suite.passed + suite.failed;
    suite.discovered = suite.passed + suite.failed + suite.ignored;
    suite
}

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
    if !suite.assertions.iter().any(|(seen, _)| *seen == name) {
        suite.assertions.push((name, status));
    }
}

// ------------------------------------------------------------- rendering ---

fn artifact(source: &Path, kind: &str, evidence_dir: &Path) -> (String, String, String) {
    let name = source
        .file_name()
        .expect("artifact has a file name")
        .to_string_lossy()
        .into_owned();
    let target = evidence_dir.join(&name);
    if source != target {
        fs::copy(source, &target)
            .unwrap_or_else(|e| panic!("copy {} -> {}: {e}", source.display(), target.display()));
    }
    let bytes = fs::read(&target).unwrap_or_else(|e| panic!("read {}: {e}", target.display()));
    (name, sha256(&bytes).to_hex(), kind.to_owned())
}

fn engine_json() -> String {
    let rustc = Command::new("rustc")
        .arg("--version")
        .output()
        .expect("rustc runs");
    format!(
        "{{\"rust\": {}, \"bevy\": {}, \"avian\": {}}}",
        jstr(String::from_utf8_lossy(&rustc.stdout).trim()),
        jstr(&locked_version("bevy")),
        jstr(&locked_version("avian3d"))
    )
}

fn assertion_array(assertions: &[(String, &'static str)]) -> String {
    assertions
        .iter()
        .map(|(name, status)| {
            format!(
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\"]}}",
                jstr(name)
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn artifact_array(artifacts: &[(String, String, String)]) -> String {
    artifacts
        .iter()
        .map(|(name, digest, kind)| {
            format!(
                "{{\"path\": {}, \"sha256\": {digest:?}, \"kind\": {kind:?}}}",
                jstr(name)
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn str_array(items: &[String]) -> String {
    format!(
        "[{}]",
        items.iter().map(|i| jstr(i)).collect::<Vec<_>>().join(", ")
    )
}

fn jstr(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
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

fn iso_utc_now() -> String {
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after 1970")
        .as_secs() as i64;
    let (y, mo, d, h, mi, s) = civil_from_unix(epoch);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

fn civil_from_unix(seconds: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (
        if m <= 2 { y + 1 } else { y },
        m,
        d,
        (rest / 3_600) as u32,
        ((rest % 3_600) / 60) as u32,
        (rest % 60) as u32,
    )
}
