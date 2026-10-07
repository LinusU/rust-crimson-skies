//! Acceptance for `M01-LC-DIRECTIVE-MEANING` (#675), the integration step the
//! split left behind: stages A–D recorded what M01's directive keys do
//! (`docs/findings/2026-10-04-m01-lc-mission-program.md`,
//! `docs/findings/2026-10-06-m01-lc-directive-{b,c,d}-*.md`) and stage E
//! (#683) turned those recordings into the `DirectiveDisposition::Measured`
//! table in `cs_content::mission_control`. This suite pins the **join** those
//! stages only meet in one tree, which no sibling suite can check on its own:
//!
//! * every directive key reachable in `zbd/c1c/m01` reports a disposition —
//!   `Measured` or `TerminalOutcome`, never `MeaningNotMeasured` — with the
//!   block, site and key counts the mission-program finding recorded;
//! * every `Measured` disposition **cites a findings document the repository
//!   actually holds**, so the "record findings" half of the task is load
//!   bearing: a citation naming a document nobody wrote, or a recorded stage
//!   finding nothing cites, fails the run. That link is the production table's
//!   only connection to its evidence, and no sibling suite reads it;
//! * the launch gate stays where stage E left it: `mission_program` /
//!   `mission_objectives` support is `MeasuredControlRecord::is_complete()`
//!   (`cs_app::mission_control`'s census surfaces), which reads the lowering
//!   requirements — now derived from the record's own lowering attempt — so a
//!   mission whose reachable vocabulary is unmeasured, or whose attempt
//!   refused, still reports Unsupported.
//!
//! `crates/cs_app/src/mission_launch.rs` (task #359, where
//! `plan_mission_launch` spells those two surfaces by name) is not on this
//! branch — the census rows and their lowering accounting are the surfaces
//! that exist here, and they are what these tests read.
//!
//! Native-side claims (handler addresses, field offsets, what each operation
//! does) are static code evidence from the stage A–D readings of
//! `crimson.decrypted.exe` and cannot be re-derived by a test; nothing here is
//! `verified_original`. The two tests that read `$CS_GAME_DIR` need
//! `CS_GAME_DIR` and are ignored without it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use cs_app::control_lowering::lower_control_record;
use cs_app::mission_control::survey_mission_control_programs;
use cs_content::mission_control::{
    DirectiveDisposition, LoweringRequirementKind, TerminalOutcome, measure_control_record,
    measured_directive,
};
use cs_content::stunts::ZrdValue;
use cs_types::content::{ContentId, ContentKind};

const M01: &str = "zbd/c1c/m01";

/// The findings documents this chain rests on: the corpus measurement the
/// split started from (`2026-10-04-m01-lc-mission-program`) plus the four
/// stage documents the split recorded. The disposition table's `evidence`
/// cites slugs under `docs/findings/`; the parser map (`directive-a`) is the
/// source of the parse sites and argument shapes the stages read rather than
/// a per-key effect citation, so it is required to be *held* here and only
/// *cited* where a key's measurement names it.
const RECORDED_FINDINGS: [&str; 5] = [
    "2026-10-04-m01-lc-mission-program",
    "2026-10-06-m01-lc-directive-a-objective-directive-parser",
    "2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics",
    "2026-10-06-m01-lc-directive-c-ai-world-and-animation-directives",
    "2026-10-06-m01-lc-directive-d-sound-help-timer-directives",
];

/// The findings M01's own keys cite: the corpus-wide parser measurement
/// (`2026-10-04-m01-lc-mission-program`) is cited by keys M01 does not spell
/// (`DANGER_ZONES_*`), so M01's measured vocabulary is fed by B, C and D only.
const M01_FINDINGS: [&str; 3] = [
    "2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics",
    "2026-10-06-m01-lc-directive-c-ai-world-and-animation-directives",
    "2026-10-06-m01-lc-directive-d-sound-help-timer-directives",
];

/// The findings some key in the installation is measured from, and which the
/// table therefore may not stop citing without leaving a recorded
/// measurement with nothing behind it.
const REQUIRED_CITATIONS: [&str; 4] = [
    "2026-10-04-m01-lc-mission-program",
    "2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics",
    "2026-10-06-m01-lc-directive-c-ai-world-and-animation-directives",
    "2026-10-06-m01-lc-directive-d-sound-help-timer-directives",
];

/// The directive keys `crimson.decrypted.exe`'s parser vocabulary carries and
/// **no mission in the census spells**: stage A's "Parser keys present but NOT
/// spelled by M01" list (`DELETE_ON_SUCCESS` only ever as a `TRAVELERS`
/// argument token). They therefore have no disposition in any census row —
/// they are not corpus refusals — and [`measured_directive`] must keep
/// answering `None` for them, so spelling one would refuse it.
const PARSER_ONLY_KEYS: [&str; 7] = [
    "OBJECTIVE_HD_a",
    "OBJECTIVE_HD_b",
    "TEST_COMPLETE",
    "COMPLETION_COUNT",
    "WIN_ANIM",
    "LOSS_ANIM",
    "DELETE_ON_SUCCESS",
];

/// The keys some mission in the census spells that no finding covers: the six
/// live `Unmeasured { MeaningNotMeasured }` refusals the corpus carries.
const CORPUS_REFUSED_KEYS: [&str; 6] = [
    "Change",
    "SET_AI_",
    "WAKEUP_OBJECTIVE_WHEN_I_COMPLETE",
    "mobile",
    "net",
    "to",
];

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR: retail acceptance needs the install"),
    )
}

/// The repository root, so a citation can be resolved to the file it names.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root is where the crate lives")
}

/// Every citation `evidence` must resolve: `docs/findings/<slug>.md`, present
/// and non-empty.
fn resolve_citations<'a>(
    root: &Path,
    context: &str,
    citations: impl IntoIterator<Item = &'a str>,
) -> BTreeSet<&'a str> {
    let cited: BTreeSet<&str> = citations.into_iter().collect();
    assert!(
        !cited.is_empty(),
        "{context}: a measured disposition names its evidence"
    );
    for slug in &cited {
        let path = root.join("docs/findings").join(format!("{slug}.md"));
        let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!(
                "{context}: the table cites `{slug}`, which the repository does not record: \
                 {} ({error})",
                path.display()
            )
        });
        assert!(
            !text.trim().is_empty(),
            "{context}: the cited finding `{slug}` is empty"
        );
    }
    cited
}

/// **M01's whole reachable vocabulary is measured, and every measurement names
/// a finding the repository holds.**
///
/// Re-derived from the installation on every run: the control member is found
/// by its numbered blocks, walked by production code, and every key's
/// disposition is the production table's. The figures are the mission-program
/// finding's (58 blocks, 353 directive sites, 43 distinct keys, two terminal
/// spellings), so a document edited away from its own data or a table that
/// stops covering a key fails here.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_directive_meaning_m01s_whole_vocabulary_is_measured_and_cites_its_findings() {
    let root = workspace_root();
    let census =
        survey_mission_control_programs(&game_dir()).expect("the census runs on the installation");
    let record = census
        .row(M01)
        .unwrap_or_else(|| {
            panic!(
                "{M01} is measured; the census holds {} missions",
                census.len()
            )
        })
        .record()
        .unwrap_or_else(|| panic!("{M01} declares a control program"));

    assert_eq!(record.blocks(), 58, "M01's numbered objective blocks");
    assert_eq!(record.sites(), 353, "M01's directive sites");
    assert_eq!(record.vocabulary(), 43, "M01's distinct directive keys");
    assert!(
        record.refusals().is_empty(),
        "every block of M01's control member was read: {:?}",
        record.refusals()
    );

    let mut measured = 0usize;
    let mut terminal = 0usize;
    let mut cited = BTreeSet::new();
    let mut keys_with_unknowns = 0usize;
    let mut unknown_statements = 0usize;
    for key in record.keys() {
        match key.disposition() {
            DirectiveDisposition::Measured(directive) => {
                measured += 1;
                assert!(
                    !directive.summary.is_empty(),
                    "{}: a measured key states its effect",
                    key.key
                );
                let documents =
                    resolve_citations(&root, &key.key, directive.evidence.iter().copied());
                cited.extend(documents);
                if !directive.unknowns.is_empty() {
                    keys_with_unknowns += 1;
                }
                unknown_statements += directive.unknowns.len();
                for statement in directive.unknowns {
                    assert!(
                        !statement.is_empty(),
                        "{}: a residual unknown is a statement, not a placeholder",
                        key.key
                    );
                }
            }
            DirectiveDisposition::TerminalOutcome { outcome } => {
                terminal += 1;
                let want = match key.key.as_str() {
                    "INSTANTWIN" => TerminalOutcome::Succeeded,
                    "INSTANTLOSS" => TerminalOutcome::Failed,
                    other => panic!(
                        "{other}: the only terminal spellings in M01 are the two outcome keys"
                    ),
                };
                assert_eq!(outcome, want, "{}: its measured outcome class", key.key);
            }
            DirectiveDisposition::Unmeasured { reason } => panic!(
                "{}: reachable in {M01} and still unmeasured ({reason}) — the parent task is not \
                 done while a reachable key is",
                key.key
            ),
        }
    }
    assert_eq!(measured, 41, "the keys a stage B/C/D finding measures");
    assert_eq!(terminal, 2, "the two outcome spellings");
    assert_eq!(measured + terminal, record.keys().len(), "the partition");
    assert_eq!(
        (keys_with_unknowns, unknown_statements),
        (32, 36),
        "the residual unknowns each finding left behind are carried on the keys, never dropped"
    );
    assert_eq!(
        cited,
        M01_FINDINGS.into_iter().collect::<BTreeSet<_>>(),
        "exactly the three stage findings M01's keys are measured from"
    );
}

/// **Every recorded stage finding is held, and every citation resolves.**
///
/// The corpus-wide complement of the test above. The corpus-wide parser
/// measurement (`2026-10-04-m01-lc-mission-program`) is cited by keys M01
/// does not spell (`DANGER_ZONES_*`), so it only shows up over the whole
/// installation, while stage A's parser map is held as the document the
/// stages read rather than cited per key: a per-key `evidence` entry names
/// the finding that measured *that key's effect*.
///
/// Two failures this catches: a citation naming a document nobody wrote — a
/// claim with nothing behind it — and a recorded finding whose slug no
/// measurement anywhere rests on, which would mean the table and the record
/// have drifted apart.
///
/// The same walk also pins the corpus vocabulary the parent finding's
/// "What still has no measured effect" section rests on, so that section
/// cannot claim a key the installation spells when it does not: exactly the
/// six keys some mission spells and no finding covers stay refused, the
/// parser-only keys are spelled by no mission at all, and
/// [`measured_directive`] still refuses those.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_directive_meaning_every_recorded_stage_finding_is_held_and_cited_within() {
    let root = workspace_root();
    for slug in RECORDED_FINDINGS {
        resolve_citations(&root, "a recorded stage finding", [slug]);
    }

    let census =
        survey_mission_control_programs(&game_dir()).expect("the census runs on the installation");
    assert!(
        census.measured_len() > 0,
        "the census measured the installation"
    );

    let mut cited = BTreeSet::new();
    let mut measurements = 0usize;
    let mut vocabulary: BTreeSet<&str> = BTreeSet::new();
    let mut refused: BTreeSet<&str> = BTreeSet::new();
    for row in census.measured_rows() {
        let record = row
            .record()
            .unwrap_or_else(|| panic!("{} is measured", row.mission()));
        for (key, directive) in record.measured() {
            measurements += 1;
            let documents = resolve_citations(
                &root,
                &format!("{} {}", row.mission(), key.key),
                directive.evidence.iter().copied(),
            );
            cited.extend(documents);
        }
        for key in record.keys() {
            vocabulary.insert(key.key.as_str());
            if matches!(key.disposition(), DirectiveDisposition::Unmeasured { .. }) {
                refused.insert(key.key.as_str());
            }
        }
    }
    assert!(measurements > 0, "the corpus carries measured keys");

    assert_eq!(
        vocabulary.len(),
        57,
        "the corpus's distinct directive keys (the mission-program finding's census)"
    );
    assert_eq!(
        refused,
        CORPUS_REFUSED_KEYS.into_iter().collect::<BTreeSet<_>>(),
        "the keys some mission spells and no finding covers: exactly the corpus's live refusals, \
         no key the installation does not spell may be claimed as one"
    );
    for key in PARSER_ONLY_KEYS {
        assert!(
            !vocabulary.contains(key),
            "{key}: in the executable's parser vocabulary but spelled by no mission in the \
             census, so the parent finding may not report it as a corpus refusal"
        );
        assert_eq!(
            measured_directive(key),
            None,
            "{key}: no finding entry, so a mission that spelled it would be refused"
        );
    }

    let recorded: BTreeSet<&str> = RECORDED_FINDINGS.into_iter().collect();
    let invented: Vec<&str> = cited.difference(&recorded).copied().collect();
    assert!(
        invented.is_empty(),
        "the table cites documents this task never recorded: {invented:?}"
    );
    let required: BTreeSet<&str> = REQUIRED_CITATIONS.into_iter().collect();
    let orphaned: Vec<&str> = required.difference(&cited).copied().collect();
    assert!(
        orphaned.is_empty(),
        "recorded findings no measurement anywhere cites: {orphaned:?}"
    );
}

/// **A key no finding recorded still refuses the gate.**
///
/// The negative case, synthetic so CI runs it without the installation: one
/// record spells a measured key, a terminal key and a key whose effect
/// nothing measured (`SET_AI_`, spelled in the corpus whose handler was never
/// located). The measured key must still cite a document the repository
/// holds, the uncovered key must keep its `meaning_not_measured` refusal, and
/// the record must not report complete — so no shortcut can lift the gate
/// that the mission_program / mission_objectives surfaces read.
#[test]
fn accept_m01_lc_directive_meaning_a_key_no_finding_recorded_refuses_the_gate() {
    let root = workspace_root();
    let document = record_with(&[
        ("INSTANTWIN", None),
        ("WAKE_ANIM", Some(vec![text("runwayanim")])),
        ("SET_AI_NET", Some(vec![list(vec![text("a"), text("b")])])),
        ("SET_AI_", Some(vec![text("x")])),
    ]);
    let record = measure_control_record(&document);

    assert_eq!(
        record
            .implemented()
            .iter()
            .map(|(key, outcome)| (key.key.as_str(), *outcome))
            .collect::<Vec<_>>(),
        [("INSTANTWIN", TerminalOutcome::Succeeded)],
        "the outcome spelling is the only directive the engine may act on"
    );

    let mut measured = Vec::new();
    for (key, directive) in record.measured() {
        resolve_citations(&root, &key.key, directive.evidence.iter().copied());
        measured.push(key.key.as_str());
    }
    assert_eq!(
        measured,
        ["SET_AI_NET", "WAKE_ANIM"],
        "the keys a finding records report their measured effect"
    );

    let unmeasured: Vec<(&str, &str)> = record
        .unmeasured()
        .iter()
        .map(|(key, disposition)| {
            let reason = disposition
                .refusal()
                .expect("an unmeasured disposition carries its reason");
            (key.key.as_str(), reason.code())
        })
        .collect();
    assert_eq!(
        unmeasured,
        [("SET_AI_", "meaning_not_measured")],
        "exactly the one key no finding records, refused by its measured reason"
    );

    // The lowering rows derive from the record's real lowering attempt: the
    // measured sites bind, the block's condition lowers (it spells no
    // evaluator, so it is the measured wake gate), and only the unmeasured
    // key's site keeps the calls row unmet.
    let lowered = lower_control_record(
        ContentId::from_source(ContentKind::Mission, "accept-mission")
            .map_err(|error| error.to_string()),
        "accept-mission",
        &document,
        &record,
    );
    let lowering = record.lowering(lowered.attempt());
    assert_eq!(
        lowering
            .unmet()
            .map(|row| row.kind)
            .collect::<Vec<LoweringRequirementKind>>(),
        [LoweringRequirementKind::CallArguments],
        "measured is not support: the unmeasured key's site refuses its call"
    );
    assert!(
        lowering
            .unmeasured_fields()
            .iter()
            .any(|field| field.contains("SET_AI_")),
        "the unmet row names the key no finding covers"
    );
    assert!(
        !record.is_complete(lowered.attempt()) && !lowering.complete(),
        "the surfaces mission_program / mission_objectives read stay Unsupported while a \
         reachable key is unmeasured"
    );
}

// ------------------------------------------------------- the .zrd authoring ---

fn text(value: &str) -> ZrdValue {
    ZrdValue::Text(value.to_owned())
}

fn list(children: Vec<ZrdValue>) -> ZrdValue {
    ZrdValue::List(children)
}

/// One record's fields: `key`, `value`, `key`, `value`, … flattened into the
/// document the production walk reads.
fn control_record(fields: Vec<(String, ZrdValue)>) -> ZrdValue {
    let mut children = Vec::new();
    for (key, value) in fields {
        children.push(text(&key));
        children.push(value);
    }
    list(vec![list(children)])
}

/// A single numbered block whose directives are `key` followed, if and only if
/// it has one, by its argument list.
fn record_with(directives: &[(&str, Option<Vec<ZrdValue>>)]) -> ZrdValue {
    let mut children = Vec::new();
    for (key, args) in directives {
        children.push(text(key));
        if let Some(args) = args {
            children.push(list(args.clone()));
        }
    }
    control_record(vec![("OBJECTIVE1".to_owned(), list(children))])
}
