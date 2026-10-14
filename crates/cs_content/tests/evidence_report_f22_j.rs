//! Evidence-report harness for task F22-J (#505), `docs/contracts/CLI-EVIDENCE.md`
//! and `schemas/evidence.schema.json`. Not named `accept_f22_j_*`: it is not
//! part of the acceptance suite and fails loudly when its inputs are missing.
//!
//! 1. `cargo test --workspace --locked -- accept_f22_j_ --include-ignored 2>&1 |
//!    tee private/evidence/F22-J/cargo-test.log` (note the exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F22-J \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f22_j_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_content --test evidence_report_f22_j -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py private/evidence/F22-J/acceptance.json
//!    --artifact-root private/evidence/F22-J
//!    --require-pass` (expected to exit 3 with `Unresolved issues`: the runtime
//!    questions and the owner-analyzed rules are the deliverable's unknowns and
//!    must not be deleted to turn the flag green).
//! 4. Commit a copy as `docs/findings/evidence/F22-J.json`.
//!
//! `bindings.json` is the recovered default table (command ids, label ids, key
//! codes, joystick and mouse buttons, registration addresses), the analog
//! defaults, the Japanese-keyboard remap, the recorded comparison against
//! `ActionMap::designed_default`, the cited spans and the fingerprints — ids,
//! codes, addresses, spans and digests only, never original display text.

#[path = "f22_j_support/mod.rs"]
mod support;

#[path = "f22_h_support/mod.rs"]
mod f22_h;

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};
use cs_content::config::{StringCatalog, StringLookup};
use cs_content::coordinates::{ORIGINAL_IMAGE_SHA256, load_engine_image};
use cs_formats::ParseContext;
use cs_types::asset_id::SourceSpan;
use cs_types::input::{
    ActionMap, DefaultAgreement, DefaultSubject, ORIGINAL_ANALOG_DEFAULTS_SPAN,
    ORIGINAL_BINDING_STORE_VA, ORIGINAL_DEFAULT_BINDINGS, ORIGINAL_FLY_MODE_DEFAULT,
    ORIGINAL_FLY_MODE_DEFAULT_VA, ORIGINAL_IMAGE_VA_BIAS, ORIGINAL_JAPANESE_REMAP,
    ORIGINAL_JAPANESE_REMAP_VA, ORIGINAL_JOYSTICK_AXES, ORIGINAL_JOYSTICK_INPUT_VA,
    ORIGINAL_KEY_NAME_VA, ORIGINAL_MODIFIER_MATCH_SPAN, ORIGINAL_MOUSE_SENS_DEFAULT,
    ORIGINAL_MOUSE_SENS_DEFAULT_VA, ORIGINAL_REGISTER_VA, ORIGINAL_TABLE_SPAN,
    compare_with_original_default,
};
use f22_h::{COMMAND_LABELS, STRINGS_LANGUAGE, parse_name_id_table};
use support::*;

const RETAIL_TESTS: [&str; 2] = [
    "accept_f22_j_retail_the_image_registers_the_recorded_defaults",
    "accept_f22_j_retail_strings_dll_names_every_recorded_label",
];

const REVIEW_METHOD: &str = "Acceptance suite run locally with the retail capability; the report is derived from the recorded log, production discovery of $CS_GAME_DIR, the production engine-image loader (CS_ENGINE_IMAGE) and a second production run of the readers recorded in bindings.json. Claim is implemented only: the original's 64 default control rows (command ids, label ids, key codes, joystick and mouse buttons, registration addresses), the two analog defaults and the Japanese-keyboard remap are recorded from the owner's static analysis plus this task's bounded read-only scan of the decrypted image, and the recorded differences against ActionMap::designed_default are pinned by tests. No original executable ran, so nothing here is verified_original, and the project default stays designed. LIMITS: (1) which key actually fires which command in the running game, what text entry suppresses and what focus loss does are runtime questions (task #358 REF-OWNER-FIRST-CAPTURE); (2) the binding encoding (which argument is key A, the modifier bits, the modifier-matching and uniqueness rules) and the joystick axis mapping are the owner's static analysis, not runtime observations; (3) the project default map was not changed to the original's bindings. The report is validated with tools/validate_evidence.py; --require-pass exits 3 on the preserved unknowns.";

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR, CS_ENGINE_IMAGE"]
fn evidence_report_f22_j_writes_the_acceptance_report() {
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

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", log_path.display()));
    let suite = parse_suite(&log);
    assert!(
        suite.passed > 0 && suite.assertions.len() as u64 >= suite.passed,
        "the acceptance log was not understood: {suite:?}"
    );
    for retail_test in RETAIL_TESTS {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| panic!("{retail_test} did not run: run with --include-ignored"));
        assert_eq!(status, "pass", "{retail_test}");
    }

    // Second observation: production discovery, the production image loader
    // and the support module's re-decode of the table.
    let found = discover(&game_dir).expect("production discovery reads the installation");
    let install = fingerprint(&found.manifest);
    let install_sha256 = install.to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let image = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    assert_eq!(image.digest.to_hex(), ORIGINAL_IMAGE_SHA256);
    let decoded = decode_registrations(&image.bytes);
    assert_eq!(decoded.len(), ORIGINAL_DEFAULT_BINDINGS.len());
    let image_rows_agree = ORIGINAL_DEFAULT_BINDINGS
        .iter()
        .zip(&decoded)
        .all(|(row, found)| {
            row.command_id == found.command_id
                && row.label_id == found.label_id
                && row.key_a.code() == found.key_a
                && row.key_b.code() == found.key_b
                && row.joystick_button == found.joystick_button
                && row.mouse_button == found.mouse_button
                && row.registration_va == found.call_va
        });
    assert!(image_rows_agree, "the recorded table must match the image");

    // The string side: every recorded label id resolves through the production
    // StringCatalog and is named in the `.data` table.
    let strings = fs::read(game_dir.join("strings.dll")).expect("strings.dll");
    let span = SourceSpan::new(install, "strings.dll", None, 0, strings.len() as u64, None)
        .expect("a valid span");
    let mut context = ParseContext::with_defaults("strings.dll");
    let catalog = StringCatalog::read(&mut context, span, &strings).expect("strings.dll reads");
    let table = parse_name_id_table(&strings);
    let mut label_items: Vec<String> = Vec::new();
    for row in ORIGINAL_DEFAULT_BINDINGS {
        let symbol = support_symbol(row.label_id);
        assert_eq!(
            table.get(symbol).copied(),
            Some(row.label_id),
            "{symbol} names its label id"
        );
        match catalog.resolve(row.label_id, Some(STRINGS_LANGUAGE)) {
            StringLookup::Found(resolved) => {
                assert!(!resolved.code_units.is_empty());
            }
            other => panic!("label {} must resolve, got {other:?}", row.label_id),
        }
        label_items.push(format!(
            "{{\"command_id\": {}, \"symbol\": {}, \"label_id\": {}, \"resolved_code_units\": {}}}",
            row.command_id,
            jstr(symbol),
            row.label_id,
            label_code_units(&catalog, row.label_id)
        ));
    }

    let comparison_items: Vec<String> =
        compare_with_original_default(&ActionMap::designed_default())
            .iter()
            .map(|row| {
                let subject = match row.subject {
                    DefaultSubject::Declared(label) => format!("declared:{}", jstr(label)),
                    DefaultSubject::OriginalOnly(id) => format!("original_only:{id}"),
                };
                format!(
                    "{{\"subject\": {}, \"agreement\": {}, \"designed\": {}, \"original\": {}}}",
                    subject,
                    jstr(agreement_label(row.agreement)),
                    str_array(&row.designed),
                    str_array(&row.original)
                )
            })
            .collect();

    let bindings_path = evidence_dir.join("bindings.json");
    fs::write(
        &bindings_path,
        format!(
            "{{\"install_sha256\": {}, \"content_sha256\": {}, \"candidate_tree\": {}, \
             \"engine_image\": {{\"sha256\": {}, \"len\": {}, \"va_bias\": {}}}, \
             \"spans\": {{\"table\": [{}, {}], \"register_call\": {}, \
             \"binding_store\": {}, \"key_name\": {}, \
             \"modifier_match\": [{}, {}], \"joystick_input\": {}, \
             \"analog_defaults\": [{}, {}]}}, \
             \"fly_mode_default\": {}, \"mouse_sens_default\": {}, \
             \"fly_mode_default_va\": {}, \"mouse_sens_default_va\": {}, \
             \"japanese_remap_va\": {}, \"japanese_remap\": [{}], \
             \"joystick_axes\": [{}], \"bindings\": [{}], \
             \"screen_only_labels\": [{}], \"not_on_screen_labels\": [{}], \
             \"comparison\": [{}], \"image_rows_agree\": {}}}\n",
            jstr(&install_sha256),
            jstr(&content_sha256),
            jstr(&candidate_tree),
            jstr(ORIGINAL_IMAGE_SHA256),
            image.bytes.len(),
            hex(ORIGINAL_IMAGE_VA_BIAS),
            hex(ORIGINAL_TABLE_SPAN.0),
            hex(ORIGINAL_TABLE_SPAN.1),
            hex(ORIGINAL_REGISTER_VA),
            hex(ORIGINAL_BINDING_STORE_VA),
            hex(ORIGINAL_KEY_NAME_VA),
            hex(ORIGINAL_MODIFIER_MATCH_SPAN.0),
            hex(ORIGINAL_MODIFIER_MATCH_SPAN.1),
            hex(ORIGINAL_JOYSTICK_INPUT_VA),
            hex(ORIGINAL_ANALOG_DEFAULTS_SPAN.0),
            hex(ORIGINAL_ANALOG_DEFAULTS_SPAN.1),
            ORIGINAL_FLY_MODE_DEFAULT,
            ORIGINAL_MOUSE_SENS_DEFAULT,
            hex(ORIGINAL_FLY_MODE_DEFAULT_VA),
            hex(ORIGINAL_MOUSE_SENS_DEFAULT_VA),
            hex(ORIGINAL_JAPANESE_REMAP_VA),
            remap_items(),
            joystick_axis_items(),
            binding_items().join(", "),
            label_items.join(", "),
            not_on_screen_items(),
            comparison_items.join(", "),
            image_rows_agree,
        ),
    )
    .expect("write bindings.json");
    assert_json_grammar(
        &fs::read_to_string(&bindings_path).expect("read bindings.json"),
        "bindings.json",
    );

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&bindings_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"F22-J\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": {},\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
        str_array(&unknowns()),
        jstr(&reviewer),
        jstr(REVIEW_METHOD),
    );
    let out = evidence_dir.join("acceptance.json");
    assert_json_grammar(&report, "acceptance.json");
    fs::write(&out, report).expect("write acceptance.json");
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report must NOT validate",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// The unresolved questions this task must not delete to satisfy a validator:
/// the runtime behaviour, the owner-analyzed rules and the project default's
/// status.
fn unknowns() -> Vec<String> {
    vec![
        "Which key, joystick button or mouse control actually fires which command in the running game, what the original does while text is entered and what it does on focus loss are runtime questions; they need an owner-supplied original run (task #358 REF-OWNER-FIRST-CAPTURE).".to_owned(),
        "The default rows are static code evidence: each registration call's arguments were decoded from the image bytes, but the binding encoding itself (which argument is key A, the 11-bit modifier layout, the modifier-matching rule that a plain key is ignored when a modified combination of it is bound, and the per-slot uniqueness rule) is the owner's static analysis, not a runtime observation.".to_owned(),
        "The joystick axis mapping (X to roll inverted, Y to pitch, Z or slider 0 to the throttle, Rz to rudder inverted, hat to the look commands) and the FlyMode input-mode flags (0 to 1, 1 to 3, 2 to 5) are the owner's static analysis over 0x487460 and 0x440500; the temporary in-flight remap of commands 1 and 2 (0x450550/0x450e30) is recorded but not exercised.".to_owned(),
        "The project's own default map is unchanged and stays designed: the comparison in bindings.json records where the two defaults differ, and none of the original's bindings was wired into the flight path.".to_owned(),
    ]
}

/// The recorded binding rows as JSON items.
fn binding_items() -> Vec<String> {
    ORIGINAL_DEFAULT_BINDINGS
        .iter()
        .map(|row| {
            format!(
                "{{\"command_id\": {}, \"label_id\": {}, \"category\": {}, \"key_a\": {}, \
                 \"key_b\": {}, \"joystick_button\": {}, \"mouse_button\": {}, \
                 \"registration_va\": {}}}",
                row.command_id,
                row.label_id,
                jstr(row.category.label()),
                hex(row.key_a.code()),
                hex(row.key_b.code()),
                row.joystick_button,
                row.mouse_button,
                hex(row.registration_va)
            )
        })
        .collect()
}

/// The Japanese-keyboard remap pairs as JSON items.
fn remap_items() -> String {
    ORIGINAL_JAPANESE_REMAP
        .iter()
        .map(|(input, remapped)| {
            format!(
                "{{\"input\": {}, \"remapped\": {}}}",
                hex(input),
                hex(remapped)
            )
        })
        .collect::<Vec<String>>()
        .join(", ")
}

/// The joystick axis mapping as JSON items.
fn joystick_axis_items() -> String {
    ORIGINAL_JOYSTICK_AXES
        .iter()
        .map(|binding| {
            format!(
                "{{\"axis\": {}, \"flight_axis\": {}, \"inverted\": {}}}",
                jstr(match binding.axis {
                    cs_types::input::OriginalJoystickAxis::X => "x",
                    cs_types::input::OriginalJoystickAxis::Y => "y",
                    cs_types::input::OriginalJoystickAxis::Z => "z",
                    cs_types::input::OriginalJoystickAxis::Rz => "rz",
                    cs_types::input::OriginalJoystickAxis::Hat => "hat",
                }),
                match binding.flight_axis {
                    Some(command) => jstr(command.label()),
                    None => "null".to_owned(),
                },
                binding.inverted
            )
        })
        .collect::<Vec<String>>()
        .join(", ")
}

/// The five measured labels the controls screen does not enumerate, as JSON
/// items recomputed against F22-H's measured list.
fn not_on_screen_items() -> String {
    let screen: Vec<u32> = ORIGINAL_DEFAULT_BINDINGS
        .iter()
        .map(|row| row.label_id)
        .collect();
    COMMAND_LABELS
        .iter()
        .filter(|(_, id)| !screen.contains(id))
        .map(|(symbol, id)| format!("{{\"symbol\": {}, \"label_id\": {}}}", jstr(symbol), id))
        .collect::<Vec<String>>()
        .join(", ")
}

/// The symbol `strings.dll`'s `.data` table gives label id `id`, from F22-H's
/// measured command labels or the four screen-only labels.
fn support_symbol(id: u32) -> &'static str {
    COMMAND_LABELS
        .iter()
        .find(|(_, label_id)| *label_id == id)
        .map(|(symbol, _)| *symbol)
        .or_else(|| {
            SCREEN_ONLY_LABELS
                .iter()
                .find(|(_, label_id)| *label_id == id)
                .map(|(symbol, _)| *symbol)
        })
        .unwrap_or_else(|| panic!("label {id} has no recorded symbol"))
}

/// The number of code units the production reader decodes for label `id`.
fn label_code_units(catalog: &StringCatalog, id: u32) -> usize {
    match catalog.resolve(id, Some(STRINGS_LANGUAGE)) {
        StringLookup::Found(row) => row.code_units.len(),
        other => panic!("label {id} must resolve, got {other:?}"),
    }
}

/// The stable label of an agreement class.
fn agreement_label(agreement: DefaultAgreement) -> &'static str {
    match agreement {
        DefaultAgreement::Same => "same",
        DefaultAgreement::Partial => "partial",
        DefaultAgreement::Different => "different",
        DefaultAgreement::ProjectOnly => "project_only",
        DefaultAgreement::OriginalOnly => "original_only",
        DefaultAgreement::RuntimeOnly => "runtime_only",
    }
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_content/tests/evidence_report_f22_j.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/F22-J` written relative to the
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
/// `accept_f22_j_` tests from a recorded `cargo test` output.
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
            if !name.starts_with("accept_f22_j_") {
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
    (name, sha256(&bytes).to_hex(), kind.to_owned())
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
                 \"bindings.json\"]}}",
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

/// Checks that `document` is structurally JSON: outside a string literal it
/// may hold only structure, number characters and the letters of `true`,
/// `false` and `null`.
///
/// The renderer quotes every address and key code, and this is the guard that
/// keeps it doing so: an unquoted `0x…` is valid in Rust's `format!` but not
/// in JSON, and it would otherwise reach an artifact the report advertises as
/// `kind: json`. It is deliberately not a full parser — `serde_json` is not a
/// dependency of this crate — it covers exactly the defect this document can
/// produce.
fn assert_json_grammar(document: &str, name: &str) {
    let mut in_string = false;
    let mut escaped = false;
    for (index, character) in document.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            _ if character.is_ascii_whitespace() || "{}[],:".contains(character) => {}
            _ if character.is_ascii_digit() || "-+.eE".contains(character) => {}
            't' | 'r' | 'u' | 'e' | 'f' | 'a' | 'l' | 's' | 'n' => {}
            other => panic!(
                "{name}: character {other:?} at byte {index} is not JSON outside a string — \
                 an address or key code escaped its quotes"
            ),
        }
    }
    assert!(!in_string, "{name}: an unterminated string");
}

/// A JSON string holding one address or key code in hexadecimal: JSON has no
/// hex number literal, so the value is quoted and the `0x` prefix kept.
fn hex(value: impl std::fmt::LowerHex) -> String {
    jstr(&format!("{value:#x}"))
}

/// A JSON string literal: quoted and escaped, so no report field can break
/// out of its string.
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

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to a UTC
/// calendar date, because `std` has no date formatting.
fn civil_from_unix(seconds: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (
        if m <= 2 { y + 1 } else { y },
        m as u32,
        d as u32,
        (rest / 3600) as u32,
        ((rest % 3600) / 60) as u32,
        (rest % 60) as u32,
    )
}
