//! Evidence-report harness for task `M01-LC-DIRECTIVES` (#641):
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`.
//! Not named `accept_m01_lc_directives_*`: it is not part of the acceptance
//! suite and fails loudly when its inputs are missing.
//!
//! 1. `cargo test --workspace --locked -- accept_m01_lc_directives_
//!    --include-ignored 2>&1 | tee
//!    private/evidence/M01-LC-DIRECTIVES/cargo-test.log` (note the exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/M01-LC-DIRECTIVES \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_lc_directives_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_app --test evidence_report_m01_lc_directives -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py
//!    private/evidence/M01-LC-DIRECTIVES/acceptance.json
//!    --artifact-root private/evidence/M01-LC-DIRECTIVES --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/M01-LC-DIRECTIVES.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR` and `Cargo.lock`. The second artifact is a
//! **second production observation**: the harness re-runs
//! [`survey_mission_control_programs`] over the installation and records, for
//! each of the task's four items, the verdict the record's own data carries —
//! `IDENTITY`'s measured disposition beside the zero-based index every
//! `RawObjective`'s `SymbolId` reports, every `INACTIVE<n>` key's operation,
//! every nested-shape key's shapes against the shapes the lowered calls
//! actually carry, every disagreeing key's shapes against the signatures the
//! registry registered — plus the corpus gate and every unhonoured key, named.
//! That is a real production run over the owner's installation, not a
//! paraphrase of the acceptance assertions, and it carries no original bytes:
//! keys, digests, counts, shape labels, operation codes and verdicts only.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::control_lowering::LoweredControlRecord;
use cs_app::mission_control::{RetailControlCensus, survey_mission_control_programs};
use cs_assets::install::{content_fingerprint, discover, fingerprint};
use cs_content::mission_control::{
    DirectiveDisposition, DirectiveOperation, DirectiveShape, MeasuredArg, MeasuredControlRecord,
};
use cs_content::objectives::is_objective_inactive_stage;
use cs_script::bindings::Lowering;
use cs_script::ir::{Condition, Value};

/// Every acceptance test the report must see pass: the prefix is unique to
/// this task, so the report can claim the selected assertions are exactly
/// this task's tests.
const ACCEPTANCE_PREFIXES: [&str; 1] = ["accept_m01_lc_directives_"];

/// The findings documents this task's disposition and lowering chain rests
/// on, one entry per slug, so the artifact records which of them the
/// candidate tree actually holds — `#630`'s census, the five directive
/// stages, the meaning and lowering findings that resolved the four items,
/// the mission-language record keys and this task's own closing note.
const RECORDED_FINDINGS: [&str; 10] = [
    "2026-10-04-m01-lc-mission-program",
    "2026-10-06-m01-lc-directive-a-objective-directive-parser",
    "2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics",
    "2026-10-06-m01-lc-directive-c-ai-world-and-animation-directives",
    "2026-10-06-m01-lc-directive-d-sound-help-timer-directives",
    "2026-10-06-m01-lc-directive-meaning",
    "2026-10-07-m01-lc-directive-lowering-adapter",
    "2026-10-07-m01-lc-directive-lowering-integration",
    "2026-10-12-m01-b-obj-4-mission-language-directive-keys",
    "2026-10-13-m01-lc-directives",
];

/// The M01 row label the census publishes.
const M01: &str = "zbd/c1c/m01";

/// The measured shape of one emitted `Value` — the mirror of the adapter's
/// `zrd_to_value`, so a call's structure can be compared field for field
/// against the shapes the census measured its key spelling.
fn shape_of_value(value: &Value) -> MeasuredArg {
    match value {
        Value::Int(_) => MeasuredArg::Int,
        Value::Float(_) => MeasuredArg::Float,
        Value::Str(_) => MeasuredArg::Text,
        Value::List(children) if children.is_empty() => MeasuredArg::Empty,
        Value::List(children) => MeasuredArg::List(children.iter().map(shape_of_value).collect()),
        other => panic!("a converted .zrd argument produced {other:?}, which no site spells"),
    }
}

/// Whether an emitted call's argument shape is one the census measured its
/// key spelling — the acceptance suite's own rule, applied here to the
/// observation so a report cannot carry an emitted shape the record never
/// spelled.
fn emitted_shape_is_measured(spelled: &[(DirectiveShape, u32)], emitted: &DirectiveShape) -> bool {
    if spelled.iter().any(|(shape, _)| shape == emitted) {
        return true;
    }
    match emitted {
        DirectiveShape::Arguments(args) if args.len() == 1 => match &args[0] {
            MeasuredArg::List(children) => spelled.iter().any(|(shape, _)| {
                matches!(shape, DirectiveShape::Arguments(spelled_args) if spelled_args == children)
            }),
            MeasuredArg::Empty => spelled.iter().any(|(shape, _)| {
                matches!(shape, DirectiveShape::Arguments(spelled_args) if spelled_args.is_empty())
            }),
            _ => false,
        },
        DirectiveShape::Bare => spelled.iter().any(|(shape, _)| {
            matches!(shape, DirectiveShape::Arguments(spelled_args) if spelled_args.is_empty())
        }),
        _ => false,
    }
}

/// The emitted shape of every `RawCall` a record's lowered program carries,
/// grouped by the call's key — the calls' side of the shape correspondence.
fn emitted_shapes(
    record: &MeasuredControlRecord,
    lowered: &LoweredControlRecord,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut emitted: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let raw = lowered.raw_program().expect("a complete record assembles");
    for objective in &raw.objectives {
        for call in &objective.calls {
            let shape = if call.args.is_empty() {
                DirectiveShape::Bare
            } else {
                DirectiveShape::Arguments(call.args.iter().map(shape_of_value).collect())
            };
            let key = record
                .key(&call.name)
                .expect("the call's key is in the record");
            assert!(
                emitted_shape_is_measured(&key.shapes, &shape),
                "`{}`: emitted shape {} is none of the measured shapes {:?}",
                call.name,
                shape.label(),
                key.shapes
            );
            emitted
                .entry(call.name.clone())
                .or_default()
                .insert(shape.label());
        }
    }
    emitted
}

/// How many of the lowered program's objectives carry the named condition
/// variant somewhere in their condition tree — the count the `INACTIVE`
/// ladder's lowered evaluators report.
fn conditions_carrying(
    lowered: &LoweredControlRecord,
    predicate: impl Fn(&Condition) -> bool,
) -> usize {
    fn walk(condition: &Condition, predicate: &dyn Fn(&Condition) -> bool, found: &mut bool) {
        if predicate(condition) {
            *found = true;
        }
        match condition {
            Condition::All(inner) | Condition::Any(inner) => {
                for child in inner {
                    walk(child, predicate, found);
                }
            }
            Condition::Not(inner) => walk(inner, predicate, found),
            _ => {}
        }
    }
    let program = lowered.program().expect("a complete record validates");
    let mut count = 0;
    for objective in &program.objectives {
        let mut found = false;
        walk(&objective.condition, &predicate, &mut found);
        count += usize::from(found);
    }
    count
}

/// The numbers the review method reports, each derived from the census this
/// run produced (never written down by hand).
struct Observed {
    missions: usize,
    measured: usize,
    complete: usize,
    absent: usize,
    campaign_ready: bool,
    m01_blocks: u32,
    m01_sites: u32,
    m01_keys: usize,
    m01_measured: usize,
    m01_terminal: usize,
    m01_unmeasured: usize,
    m01_stages: usize,
    m01_nested_keys: usize,
    m01_disagreeing_keys: usize,
    m01_inactive_blocks: usize,
    m01_calls: usize,
    m01_validated: bool,
    corpus_unmeasured_keys: usize,
    unmet_rows: usize,
    own_tests: usize,
}

/// How this run was reviewed, with every measured number **derived** from the
/// census this same run produced rather than written down.
fn review_method(observed: &Observed) -> String {
    let Observed {
        missions,
        measured,
        complete,
        absent,
        campaign_ready,
        m01_blocks,
        m01_sites,
        m01_keys,
        m01_measured,
        m01_terminal,
        m01_unmeasured,
        m01_stages,
        m01_nested_keys,
        m01_disagreeing_keys,
        m01_inactive_blocks,
        m01_calls,
        m01_validated,
        corpus_unmeasured_keys,
        unmet_rows,
        own_tests,
    } = *observed;
    format!(
        "Acceptance suite run locally with the retail capability; this harness derives every field \
         from the recorded log, production discovery of $CS_GAME_DIR, and a second production run \
         of cs_app::mission_control::survey_mission_control_programs over the installation \
         (m01-directives.json). Claim is implemented only. MEASURED: the task's four items are \
         each discharged by the findings the candidate tree holds. Item 1, IDENTITY - measured as \
         presentation data (PresentationIdentity: the class spelling selects the completion sound \
         channel and HUD class, the integer is the HUD slot ordinal); block identity is the \
         zero-based record index every cross-objective directive spells, which is what each of \
         the {m01_blocks} lowered RawObjective's SymbolId carries. Item 2, the inactive ladder - \
         every one of M01's {m01_stages} INACTIVE<n> keys is measured InactiveMembers beside \
         INACTIVE_COMPLETION_COUNT's InactiveThreshold, and {m01_inactive_blocks} of the \
         {m01_blocks} lowered conditions carry Condition::InactiveMembers with the threshold \
         defaulting to the block's own member count. Item 3, nested argument shapes - the IR's \
         Value::List carries them structure-for-structure: {m01_nested_keys} M01 keys spell a \
         nested shape (ANIM_STATE, SET_HELP_LABEL, ADD_OBJECTIVE_TARGET, SET_AI_NET among them) \
         and every emitted RawCall's shape is one of the shapes the census measured its key \
         spelling, never a flattened positional row. Item 4, disagreeing shapes - {m01_disagreeing_keys} \
         M01 keys spell more than one shape and each stays Measured with one registered signature \
         per measured shape; no plurality is resolved. HARD CONSTRAINTS: over M01's {m01_keys} \
         keys ({m01_measured} measured + {m01_terminal} terminal + {m01_unmeasured} unmeasured), \
         all {m01_sites} sites bound into {m01_calls} call verdict(s), validation reports \
         {m01_validated} and is_complete() is true - while across the corpus {corpus_unmeasured_keys} \
         distinct keys over {measured} measured row(s) of {missions} mission-scoped reader(s) \
         still carry a named Unmeasured reason, {unmet_rows} lowering row(s) stay unmet \
         corpus-wide, {absent} reader(s) declare no control program and campaign_ready() is \
         {campaign_ready} with {complete} mission(s) complete. LIMITS OF WHAT WAS MEASURED: this \
         is a static lowering of the record into the IR - no original program was run and no \
         mission was played, so nothing here is verified_original behaviour, and a bound \
         Action::Directive emits the measured operation to the host without proving a world-side \
         handler exists. TEST-SELECTION NOTE: the prefix accept_m01_lc_directives_ is unique to \
         this task, so the {own_tests} discovered assertions are exactly this task's tests. \
         Validated with tools/validate_evidence.py --require-pass."
    )
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m01_lc_directives_writes_the_acceptance_report() {
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

    // The second production observation: the census re-run, with every one
    // of the task's four items re-derived from the record's own data.
    let census = survey_mission_control_programs(&game_dir).expect("the census surveys");
    let row = census.row(M01).expect("M01 is present");
    let record = row.record().expect("M01 declares a control program");
    let lowered = row
        .lowering_attempt()
        .expect("a measured row carries the lowering attempt");
    let attempt = lowered.attempt();

    assert_eq!(
        (record.blocks(), record.sites(), record.vocabulary()),
        (58, 353, 43),
        "the task's figures for M01: blocks, directive sites and distinct keys"
    );
    let m01_measured = record.measured().len();
    let m01_terminal = record.implemented().len();
    let m01_unmeasured = record.unmeasured().len();
    assert_eq!(
        m01_measured + m01_terminal + m01_unmeasured,
        record.keys().len(),
        "the three dispositions partition M01's vocabulary"
    );
    assert_eq!(
        m01_unmeasured,
        0,
        "M01's whole vocabulary is honoured: {:?}",
        record
            .unmeasured()
            .iter()
            .map(|(key, _)| key.key.as_str())
            .collect::<Vec<_>>()
    );

    // Item 1: `IDENTITY` measured presentation data, with the zero-based
    // index carrying each objective's identity.
    let identity = record.key("IDENTITY").expect("M01 spells IDENTITY");
    assert!(
        matches!(
            identity.disposition(),
            DirectiveDisposition::Measured(directive)
                if directive.operation == DirectiveOperation::PresentationIdentity
        ),
        "IDENTITY is presentation data, not the objective identity: {:?}",
        identity.disposition()
    );
    let raw = lowered.raw_program().expect("a complete record assembles");
    for (index, objective) in raw.objectives.iter().enumerate() {
        assert_eq!(
            objective.id.0, index as u32,
            "each RawObjective's SymbolId is its zero-based position"
        );
    }

    // Item 2: every `INACTIVE<n>` key is the member-count evaluator, and
    // `INACTIVE_COMPLETION_COUNT` is its threshold.
    let stages: Vec<&cs_content::mission_control::MeasuredDirectiveKey> = record
        .keys()
        .iter()
        .filter(|key| is_objective_inactive_stage(&key.key))
        .collect();
    assert!(!stages.is_empty(), "M01 spells inactive stage keys");
    for stage in &stages {
        assert!(
            matches!(
                stage.disposition(),
                DirectiveDisposition::Measured(directive)
                    if directive.operation == DirectiveOperation::InactiveMembers
            ),
            "{}: the member-count evaluator: {:?}",
            stage.key,
            stage.disposition()
        );
    }
    let threshold_key = record
        .key("INACTIVE_COMPLETION_COUNT")
        .expect("M01 spells the threshold");
    assert!(
        matches!(
            threshold_key.disposition(),
            DirectiveDisposition::Measured(directive)
                if directive.operation == DirectiveOperation::InactiveThreshold
        ),
        "INACTIVE_COMPLETION_COUNT is the measured threshold: {:?}",
        threshold_key.disposition()
    );
    let m01_inactive_blocks = conditions_carrying(lowered, |condition| {
        matches!(condition, Condition::InactiveMembers { .. })
    });

    // Item 3: the keys spelling a nested shape are all measured, and every
    // emitted call's shape is one the census measured its key spelling.
    let nested_keys: Vec<&cs_content::mission_control::MeasuredDirectiveKey> = record
        .keys()
        .iter()
        .filter(|key| {
            key.shapes.iter().any(|(shape, _)| match shape {
                DirectiveShape::Arguments(args) => args
                    .iter()
                    .any(|arg| matches!(arg, MeasuredArg::List(_) | MeasuredArg::Empty)),
                _ => false,
            })
        })
        .collect();
    assert!(
        nested_keys
            .iter()
            .any(|key| key.key == "ANIM_STATE" || key.key == "SET_HELP_LABEL"),
        "M01's nested-shape keys: {:?}",
        nested_keys
            .iter()
            .map(|key| key.key.as_str())
            .collect::<Vec<_>>()
    );
    for key in &nested_keys {
        assert!(
            matches!(key.disposition(), DirectiveDisposition::Measured(_)),
            "{}: the nested shape is carried, not refused",
            key.key
        );
    }
    let emitted = emitted_shapes(record, lowered);

    // Item 4: every disagreeing key stays measured, with one registered
    // signature per measured shape.
    let disagreeing_keys: Vec<&cs_content::mission_control::MeasuredDirectiveKey> = record
        .keys()
        .iter()
        .filter(|key| key.shapes.len() > 1)
        .collect();
    for key in &disagreeing_keys {
        assert!(
            matches!(key.disposition(), DirectiveDisposition::Measured(_)),
            "{}: disagreeing shapes stay measured in M01",
            key.key
        );
        let spec = lowered
            .registry()
            .get(&key.key)
            .unwrap_or_else(|| panic!("{}: a measured key registers", key.key));
        assert_eq!(
            spec.signatures.len(),
            key.shapes.len(),
            "{}: one signature per measured shape",
            key.key
        );
    }

    // The corpus gate: every unhonoured key across every row stays counted
    // and named, and the campaign stays closed on them.
    let mut unhonored: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
    for corpus_row in census.measured_rows() {
        let corpus_record = corpus_row
            .record()
            .expect("a measured row carries a record");
        for (key, disposition) in corpus_record.unmeasured() {
            let reason = disposition
                .refusal()
                .expect("an unmeasured disposition carries a reason");
            unhonored
                .entry(key.key.clone())
                .or_insert_with(|| (reason.code().to_owned(), Vec::new()))
                .1
                .push(corpus_row.mission().to_owned());
        }
    }
    assert!(
        unhonored.contains_key("WAKEUP_OBJECTIVE_WHEN_I_COMPLETE"),
        "the corpus's unhonoured keys are still named"
    );
    let unmet_rows: usize = census
        .measured_rows()
        .map(|corpus_row| {
            corpus_row
                .lowering()
                .expect("a measured row lowers")
                .unmet()
                .count()
        })
        .sum();
    let complete = census.complete_missions();
    assert!(
        !census.campaign_ready() && complete.contains(&M01),
        "M01 is complete and the campaign gate is not: {complete:?}"
    );
    assert!(
        row.is_complete() && record.is_complete(attempt),
        "M01's row reports complete off its own attempt"
    );

    let observation_path = evidence_dir.join("m01-directives.json");
    fs::write(
        &observation_path,
        format!(
            "{}\n",
            render_observation(
                &census,
                &install_sha256,
                &candidate_tree,
                &emitted,
                &unhonored,
                unmet_rows
            )
        ),
    )
    .expect("write m01-directives.json");

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&observation_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"M01-LC-DIRECTIVES\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
        jstr(&reviewer),
        jstr(&review_method(&Observed {
            missions: census.len(),
            measured: census.measured_len(),
            complete: complete.len(),
            absent: census.archives_without_control_program().len(),
            campaign_ready: census.campaign_ready(),
            m01_blocks: record.blocks(),
            m01_sites: record.sites(),
            m01_keys: record.keys().len(),
            m01_measured,
            m01_terminal,
            m01_unmeasured,
            m01_stages: stages.len(),
            m01_nested_keys: nested_keys.len(),
            m01_disagreeing_keys: disagreeing_keys.len(),
            m01_inactive_blocks,
            m01_calls: attempt.calls.len(),
            m01_validated: attempt.validation.as_deref() == Some(&[][..]),
            corpus_unmeasured_keys: unhonored.len(),
            unmet_rows,
            own_tests: suite.discovered as usize,
        })),
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

/// The whole observation as JSON: the corpus gate with every unhonoured key
/// named, M01's attempt beside the per-item verdicts — the identity rule the
/// emitted `SymbolId`s report, the stage keys' operations, each nested-shape
/// key's measured and emitted shapes, each disagreeing key's signatures —
/// so the report's claims are re-derived from the installation rather than
/// copied from this task's own assertions.
fn render_observation(
    census: &RetailControlCensus,
    install_sha256: &str,
    candidate_tree: &str,
    emitted: &BTreeMap<String, BTreeSet<String>>,
    unhonored: &BTreeMap<String, (String, Vec<String>)>,
    unmet_rows: usize,
) -> String {
    let row = census.row(M01).expect("M01 is present");
    let record = row.record().expect("M01 declares a control program");
    let lowered = row
        .lowering_attempt()
        .expect("a measured row carries the lowering attempt");
    let attempt = lowered.attempt();
    let raw = lowered.raw_program().expect("a complete record assembles");
    let complete = census.complete_missions();

    // Item 1: the emitted identity rule — index, symbol and content id.
    let objectives_json: Vec<String> = raw
        .objectives
        .iter()
        .enumerate()
        .map(|(index, objective)| {
            format!(
                "{{\"position\": {}, \"symbol_id\": {}, \"content\": {}}}",
                index,
                objective.id.0,
                jstr(objective.content.key())
            )
        })
        .collect();

    // Item 2: the stage keys and their measured operation, beside the count
    // of blocks whose lowered condition reads the member fact.
    let stages_json: Vec<String> = record
        .keys()
        .iter()
        .filter(|key| {
            is_objective_inactive_stage(&key.key) || key.key == "INACTIVE_COMPLETION_COUNT"
        })
        .map(|key| {
            format!(
                "{{\"key\": {}, \"sites\": {}, \"disposition\": {}}}",
                jstr(&key.key),
                key.sites,
                jstr(&disposition_label(&key.disposition()))
            )
        })
        .collect();

    // Item 3 + 4: every key with its measured shapes, the signatures the
    // registry registered for it, and the shapes the emitted calls carry.
    let keys_json: Vec<String> = record
        .keys()
        .iter()
        .map(|key| {
            let shapes: Vec<String> = key
                .shapes
                .iter()
                .map(|(shape, sites)| {
                    format!(
                        "{{\"shape\": {}, \"sites\": {}}}",
                        jstr(&shape.label()),
                        sites
                    )
                })
                .collect();
            let signatures = lowered
                .registry()
                .get(&key.key)
                .map(|spec| spec.signatures.len());
            let binding = lowered
                .registry()
                .get(&key.key)
                .map(|spec| match spec.lowering {
                    Lowering::Directive(operation) => format!(
                        "{{\"lowering\": \"directive\", \"operation\": {}}}",
                        jstr(operation.code())
                    ),
                    Lowering::Finish(outcome) => {
                        format!("{{\"lowering\": \"finish\", \"outcome\": {outcome:?}}}")
                    }
                    other => format!("{{\"lowering\": {}}}", jstr(&format!("{other:?}"))),
                });
            let emitted_shapes: Vec<String> = emitted
                .get(&key.key)
                .map(|shapes| shapes.iter().map(|shape| jstr(shape)).collect())
                .unwrap_or_default();
            format!(
                "{{\"key\": {}, \"sites\": {}, \"disposition\": {}, \"shapes\": [{}], \
                 \"signatures\": {}, \"emitted_shapes\": [{}], \"binding\": {}}}",
                jstr(&key.key),
                key.sites,
                jstr(&disposition_label(&key.disposition())),
                shapes.join(", "),
                signatures.map_or("null".to_owned(), |count| count.to_string()),
                emitted_shapes.join(", "),
                binding.unwrap_or_else(|| "null".to_owned()),
            )
        })
        .collect();

    // The corpus's unhonoured keys, each named with its reason and the
    // missions that spell it.
    let unhonored_json: Vec<String> = unhonored
        .iter()
        .map(|(key, (reason, missions))| {
            format!(
                "{{\"key\": {}, \"reason\": {}, \"missions\": {}}}",
                jstr(key),
                jstr(reason),
                string_array(missions)
            )
        })
        .collect();

    let unmet_json: Vec<String> = census
        .unmet_by_requirement()
        .iter()
        .map(|(requirement, missions)| {
            format!(
                "{{\"requirement\": {}, \"missions\": {}}}",
                jstr(requirement),
                string_array(missions)
            )
        })
        .collect();

    let recorded_rows: Vec<String> = RECORDED_FINDINGS
        .iter()
        .map(|slug| {
            format!(
                "{{\"slug\": {}, \"held\": {}}}",
                jstr(slug),
                finding_held(slug)
            )
        })
        .collect();

    format!(
        "{{\"install_sha256\": {}, \"candidate_tree\": {}, \"missions\": {}, \"measured\": {}, \
         \"absent\": {}, \"complete_missions\": {}, \"campaign_ready\": {}, \"corpus\": \
         {{\"unmet_lowering_rows\": {}, \"unmet_by_requirement\": [{}], \"unhonored_keys\": \
         [{}]}}, \"m01\": {{\"mission\": {}, \"blocks\": {}, \"sites\": {}, \"vocabulary\": {}, \
         \"measured\": {}, \"terminal\": {}, \"unmeasured\": {}, \"bindings\": {}, \"objectives\": \
         {}, \"identity\": {{\"rule\": \"symbol_id_is_the_zero_based_record_index\", \
         \"objectives\": [{}]}}, \"inactive_stages\": {{\"keys\": [{}], \
         \"inactive_members_blocks\": {}}}, \"calls\": {{\"bound\": {}, \"refused\": {}}}, \
         \"unbound_keys\": {}, \"validation\": {}, \"keys\": [{}]}}, \
         \"recorded_findings\": [{}]}}",
        jstr(install_sha256),
        jstr(candidate_tree),
        census.len(),
        census.measured_len(),
        census.archives_without_control_program().len(),
        string_array(&complete.iter().map(|m| (*m).to_owned()).collect::<Vec<_>>()),
        census.campaign_ready(),
        unmet_rows,
        unmet_json.join(", "),
        unhonored_json.join(", "),
        jstr(
            &attempt
                .mission
                .as_ref()
                .map_or_else(|reason| reason.clone(), |id| id.clone())
        ),
        record.blocks(),
        record.sites(),
        record.vocabulary(),
        record.measured().len(),
        record.implemented().len(),
        record.unmeasured().len(),
        lowered.bindings(),
        attempt.objectives,
        objectives_json.join(", "),
        stages_json.join(", "),
        count_inactive_blocks(raw),
        attempt
            .calls
            .iter()
            .filter(|outcome| matches!(outcome, cs_content::mission_control::CallOutcome::Bound))
            .count(),
        attempt
            .calls
            .iter()
            .filter(|outcome| matches!(
                outcome,
                cs_content::mission_control::CallOutcome::Refused(_)
            ))
            .count(),
        string_array(&attempt.unbound_keys),
        match &attempt.validation {
            None => "null".to_owned(),
            Some(errors) => string_array(errors),
        },
        keys_json.join(", "),
        recorded_rows.join(", "),
    )
}

/// One disposition as a short label — `terminal:<outcome>`, `measured:<operation>`
/// or `unmeasured:<reason>` — so the observation names every key's verdict.
fn disposition_label(disposition: &DirectiveDisposition) -> String {
    match disposition {
        DirectiveDisposition::TerminalOutcome { outcome } => {
            format!("terminal:{}", outcome.label())
        }
        DirectiveDisposition::Measured(directive) => {
            format!("measured:{}", directive.operation.code())
        }
        DirectiveDisposition::Unmeasured { reason } => {
            format!("unmeasured:{}", reason.code())
        }
    }
}

/// How many of an assembled `RawProgram`'s objective conditions carry an
/// `InactiveMembers` evaluator — the raw-side count beside
/// [`conditions_carrying`]'s bound-program count.
fn count_inactive_blocks(raw: &cs_script::bindings::RawProgram) -> usize {
    fn carries(condition: &Condition) -> bool {
        match condition {
            Condition::InactiveMembers { .. } => true,
            Condition::All(inner) | Condition::Any(inner) => inner.iter().any(carries),
            Condition::Not(inner) => carries(inner),
            _ => false,
        }
    }
    raw.objectives
        .iter()
        .filter(|objective| carries(&objective.condition))
        .count()
}

/// Whether `docs/findings/<slug>.md` exists in the candidate tree and is not
/// empty — the repository half of the citation chain.
fn finding_held(slug: &str) -> bool {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root is where the crate lives");
    fs::read_to_string(root.join("docs/findings").join(format!("{slug}.md")))
        .is_ok_and(|text| !text.trim().is_empty())
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_m01_lc_directives.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/M01-LC-DIRECTIVES` written relative
/// to the workspace root in the module doc must be re-anchored here.
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
    // A Gregorian date rendered from the epoch second, so the report needs no
    // calendar dependency.
    let days = (since / 86_400) as i64;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        (since % 86_400) / 3600,
        (since % 3600) / 60,
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

/// Extracts the per-test results of the `accept_m01_lc_directives_` tests
/// from a recorded `cargo test` output.
///
/// The counts come from the **prefixed test lines**, not from the `test
/// result:` summaries: a summary aggregates every test binary cargo ran, so
/// reading it would report hundreds of unrelated tests as this task's
/// acceptance selection. A prefixed test that was skipped is recorded with
/// the schema's `unknown` status rather than counted as a pass, so a report
/// can never claim an assertion it did not run.
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
///
/// The prefix is matched on the name's **last** path segment, because libtest
/// prints an in-module unit test under its module path. Matching the whole
/// name instead would silently drop every in-module acceptance test from
/// `discovered`, from the assertion list and from the log.
fn carries_prefix(name: &str) -> bool {
    name.rsplit("::").next().is_some_and(|segment| {
        ACCEPTANCE_PREFIXES
            .iter()
            .any(|prefix| segment.starts_with(prefix))
    })
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
                 \"m01-directives.json\"]}}",
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

/// A JSON array of strings (an alias of [`str_array`] at the call sites that
/// read as data rather than as a command line).
fn string_array(items: &[String]) -> String {
    str_array(items)
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
            other if (other as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", other as u32)),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}
