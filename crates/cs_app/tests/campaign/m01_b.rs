//! Acceptance stage M01-B (#259): implement and regress M01's
//! mission-specific compatibility gaps (`missions/M01.md`, work order
//! `M01-B`).
//!
//! **Layout correction (M01-ARCHIVE-DECISION):** the archive this suite
//! measured, `zbd/c1c/m01`, declares itself `snd_c2-NW-m1` — Northwest M1 —
//! and the corrected content-derived campaign layout binds it to work order
//! **M06**, not M01 (the true M01, `zbd/c3/m01`, declares `snd_c1-HA-m1`).
//! The mission-scoped results therefore ride on `missions/bindings/M06.json`
//! now; every measurement below still runs against the same archive, which is
//! unchanged, and the file keeps its `m01_b` name and M01-B-* task citations
//! as provenance for the work orders that produced them.
//!
//! The implementation half of this stage landed in its bounded subtasks:
//! M01-B-OBJ-1/2/3/4 measured and recovered M01's objective field families
//! (`ObjectiveRecovery`), M01-B-OPC classified M01's control member as a
//! text directive program in the F13-C probe, and M01-B-WAKE wired the
//! measured world facts into `MissionHost::step` and drove both of M01's
//! `Finish` blocks to their terminals. What the parent stage itself owns is
//! the *binding* of those results into `missions/bindings/M01.json` and the
//! campaign regressions that keep them bound:
//!
//! * `cs_content::campaign_bindings` gains a mission-scoped **checklist
//!   override** table (`mission_scoped_checklist_overrides`): the three
//!   shared checklist entries M01's measurements settled — the objective
//!   graph, script/native coverage and the failure/success precedence — are
//!   replaced on M01's record by the measured statement (residuals
//!   included), while every sibling record keeps the shared text. The
//!   committed `M01.json` is regenerated through production code, so the
//!   M01-A byte-equality pin stays green.
//! * The retail member below re-measures every number and identity the
//!   replacement entries claim — through `recover_retail_objectives` and
//!   `survey_mission_control_programs`, not from constants — and pins both
//!   directions: M01's derived and committed records carry the measured
//!   statements, M02's carry the untouched shared entries.
//! * The synthetic member pins the mechanism itself over the whole declared
//!   inventory in CI, without original data.
//!
//! Nothing here awards readiness: the record keeps `verified` false, every
//! residual is named in the entry that carries it, and ordinary-play
//! verification stays with M01-C. The retail members are
//! `#[ignore = "requires CS_GAME_DIR"]`, so CI skips them and the
//! implementer and reviewer run them with `--include-ignored`.

use std::path::PathBuf;
use std::sync::OnceLock;

use cs_app::mission_control::survey_mission_control_programs;
use cs_app::objectives::{RecordFieldFamily, recover_retail_objectives};
use cs_content::campaign_bindings::{
    MissionLabel, SourceBinding, SourceContext, mission_scoped_checklist_overrides,
    mission_scoped_unknowns, source_binding_checklist,
};
use cs_content::mission_control::DirectiveDisposition;
use cs_content::objectives_conditions::ObjectiveOutcome;
use cs_content::objectives_engine::DormantTimers;

use crate::campaign::common::{label, load_inventory, repo_path};

/// The reader archive every M01-B-* task measured — under the corrected
/// campaign layout this is M06's archive, not M01's (see the module header).
const MEASURED_MISSION: &str = "zbd/c1c/m01";

/// The checklist prefix each settled shared entry is replaced under, so a
/// reader looking for the question still finds it.
const SETTLED_QUESTIONS: [&str; 3] = [
    "objective graph",
    "script/native coverage",
    "exact failure/success precedence",
];

/// The original installation, as the environment declares it.
fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M01-B needs the retail capability; run this suite with \
             `--include-ignored` and CS_GAME_DIR pointing at the read-only installation"
        )
    }))
}

/// The source context, read once for the whole suite (fingerprinting the
/// installation walks every file, so it happens exactly once).
fn context() -> &'static SourceContext {
    static CONTEXT: OnceLock<SourceContext> = OnceLock::new();
    CONTEXT.get_or_init(|| {
        SourceContext::read(&game_dir()).expect("the installation yields a source context")
    })
}

/// One work order's binding derived from the installation, through the same
/// production path the M01-A and M06-B-FU4 suites use: the declared discovery
/// title comes from the committed inventory, never from this file.
fn binding_of(work_order: &str) -> SourceBinding {
    let title = load_inventory()
        .iter()
        .find(|(work_order_label, _)| work_order_label.as_str() == work_order)
        .map(|(_, title)| title.clone())
        .unwrap_or_else(|| panic!("the declared inventory has no {work_order} work order"));
    context()
        .bind(
            MissionLabel::new(work_order).unwrap_or_else(|error| panic!("{work_order}: {error}")),
            &title,
        )
        .unwrap_or_else(|error| panic!("{work_order} binds to the original data: {error}"))
}

/// The record's single checklist entry under `prefix`, or a loud failure.
fn entry<'a>(binding: &'a SourceBinding, prefix: &str) -> &'a String {
    let matches: Vec<&String> = binding
        .unknowns
        .iter()
        .filter(|entry| entry.starts_with(prefix))
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "{prefix:?} must appear exactly once in the derived record: {:?}",
        binding.unknowns
    );
    matches[0]
}

// ---------------------------------------------------------------- retail ---

/// **The binding record of the mission these results were measured on carries
/// them, and its siblings keep the shared checklist.**
///
/// Every number and identity the three replacement entries claim is
/// re-measured here through production code before it is pinned:
/// `recover_retail_objectives` for the objective-graph census (blocks,
/// fields, recovered/unrecovered families, the two terminal outcomes and the
/// `-1` sentinel both load behind), and `survey_mission_control_programs`
/// for the directive-program census (blocks, sites, key vocabulary and each
/// key's disposition). The measured archive is `zbd/c1c/m01` — bound to M06
/// under the corrected layout — so the derived record — and the committed
/// `missions/bindings/M06.json`, which is regenerated through production
/// code — must carry those measured statements verbatim, while M01's derived
/// and committed records keep the untouched shared entries.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_b_m06s_record_binds_the_measured_results_and_m01s_does_not() {
    // --- census 1: the objective graph, through recover_retail_objectives --
    let recovery = recover_retail_objectives(&game_dir(), MEASURED_MISSION)
        .expect("the measured archive's objective record reads through the production census");
    let blocks = recovery.blocks().len();
    let fields = recovery.fields_read();
    let recovered = recovery.fields_recovered();
    let unrecovered_by_family = recovery.unrecovered_by_family();
    let control_directives = unrecovered_by_family
        .get(&RecordFieldFamily::ControlDirective)
        .copied()
        .unwrap_or(0);
    let record_directives = unrecovered_by_family
        .get(&RecordFieldFamily::RecordDirective)
        .copied()
        .unwrap_or(0);
    let unrecovered_total = fields - recovered;
    assert_eq!(
        unrecovered_by_family.values().sum::<usize>(),
        unrecovered_total,
        "the unrecovered families partition the unrecovered remainder"
    );
    assert!(
        !unrecovered_by_family.contains_key(&RecordFieldFamily::Unmeasured),
        "every family the archive spells is measured (M01-B-OBJ)"
    );

    // The record's only outcomes are the two terminal blocks, and both load
    // dormant behind the -1 sentinel (M01-B-WAKE).
    let outcomes = recovery.outcomes();
    assert_eq!(
        outcomes.len(),
        2,
        "the archive carries exactly two outcome blocks"
    );
    let (win_block, loss_block) = {
        let mut terminals = outcomes.iter().map(|(block, outcome)| (*block, *outcome));
        let first = terminals.next().expect("at least one outcome");
        let second = terminals.next().expect("exactly two outcomes");
        assert!(terminals.next().is_none(), "exactly two outcomes");
        match (first, second) {
            ((win, ObjectiveOutcome::InstantWin), (loss, ObjectiveOutcome::InstantLoss)) => {
                (win, loss)
            }
            ((loss, ObjectiveOutcome::InstantLoss), (win, ObjectiveOutcome::InstantWin)) => {
                (win, loss)
            }
            other => panic!("the two outcomes are INSTANTWIN and INSTANTLOSS: {other:?}"),
        }
    };
    let declarations = recovery.engine_declarations();
    for block in [win_block, loss_block] {
        let declaration = declarations
            .get(&block)
            .unwrap_or_else(|| panic!("OBJECTIVE{block} carries an engine declaration"));
        let DormantTimers {
            wake_at,
            nap_after,
            rewake_after,
            sleep_at,
        } = declaration
            .begin_dormant
            .as_ref()
            .unwrap_or_else(|| panic!("OBJECTIVE{block} loads dormant"));
        assert_eq!(
            (*wake_at, *nap_after, *rewake_after, *sleep_at),
            (-1.0, -1.0, -1.0, -1.0),
            "OBJECTIVE{block} is disarmed behind the -1 sentinel on every timer slot"
        );
    }

    // --- census 2: the directive program, through the control survey -------
    let census = survey_mission_control_programs(&game_dir())
        .expect("the control census measures the installation");
    let row = census
        .row(MEASURED_MISSION)
        .expect("the census measured the archive's control program");
    let record = row.record().expect("the archive carries a control record");
    let mut measured = 0usize;
    let mut terminal = 0usize;
    let mut unknown = 0usize;
    for key in record.keys() {
        match key.disposition() {
            DirectiveDisposition::Measured(_) => measured += 1,
            DirectiveDisposition::TerminalOutcome { .. } => terminal += 1,
            DirectiveDisposition::Unmeasured { .. } => unknown += 1,
        }
    }
    assert_eq!(
        measured + terminal + unknown,
        record.keys().len(),
        "every key carries a disposition"
    );
    assert_eq!(
        record.blocks() as usize,
        blocks,
        "the two censuses agree on the block count"
    );

    // --- the derived record carries the three measured statements ----------
    let m06 = binding_of("M06");
    m06.validate()
        .expect("the derived M06 record is internally consistent");
    assert!(
        !m06.is_verified(),
        "the measured residuals keep the record unverified"
    );

    let objective_graph = entry(&m06, "objective graph: ");
    for needle in [
        "measured for this mission",
        "M01-B-OBJ",
        "M01-B-WAKE",
        "M06-C",
        "ObjectiveRecovery::program() still refuses",
        "declared-schema gap",
    ] {
        assert!(
            objective_graph.contains(needle),
            "the objective-graph entry names {needle:?}: {objective_graph}"
        );
    }
    for needle in [
        format!("declares {blocks} OBJECTIVE blocks and {fields} fields"),
        format!("{recovered} of them recovered"),
        format!("{unrecovered_total} unrecovered and named"),
        format!("{control_directives} block-level control directives"),
        format!("{record_directives} record-level directive fields"),
        format!("OBJECTIVE{win_block} spelling INSTANTWIN"),
        format!("OBJECTIVE{loss_block} spelling INSTANTLOSS"),
    ] {
        assert!(
            objective_graph.contains(&needle),
            "the objective-graph entry carries the fresh census {needle:?}: {objective_graph}"
        );
    }

    let coverage = entry(&m06, "script/native coverage: ");
    for needle in [
        "M01-B-OPC",
        "text directive program",
        "fully accounted",
        "empty-signature-table premise does not describe",
    ] {
        assert!(
            coverage.contains(needle),
            "the coverage entry names {needle:?}: {coverage}"
        );
    }
    for needle in [
        format!(
            "as {} blocks, {} directive sites and {} distinct keys",
            record.blocks(),
            record.sites(),
            record.keys().len()
        ),
        format!("of which {measured} are measured, {terminal} are terminal outcomes"),
        format!("{unknown} are unknown"),
    ] {
        assert!(
            coverage.contains(&needle),
            "the coverage entry carries the fresh directive census {needle:?}: {coverage}"
        );
    }

    let precedence = entry(&m06, "exact failure/success precedence: ");
    for needle in [
        "measured for this mission".to_owned(),
        "M01-B-WAKE".to_owned(),
        "M06-C".to_owned(),
        format!("OBJECTIVE{win_block}'s INSTANTWIN"),
        format!("OBJECTIVE{loss_block}'s INSTANTLOSS"),
        "Action::Finish".to_owned(),
        "no run reports both terminals' world facts".to_owned(),
    ] {
        assert!(
            precedence.contains(&needle),
            "the precedence entry names {needle:?}: {precedence}"
        );
    }

    // --- the committed record is the derivation ----------------------------
    let committed = std::fs::read_to_string(repo_path("missions/bindings/M06.json"))
        .expect("missions/bindings/M06.json exists");
    assert_eq!(
        committed,
        m06.to_json(),
        "the committed M06 record is not what production code derives (regenerate it with \
         CS_REGENERATE_BINDING=M06)"
    );

    // --- M01 keeps the shared checklist, derived and committed -------------
    // M01's archive is `zbd/c3/m01` under the corrected layout — none of the
    // M01-B-* measurements ran against it, so its record must not carry them.
    let m01 = binding_of("M01");
    m01.validate()
        .expect("the derived M01 record is internally consistent");
    for shared in [
        "objective graph: not bound from original data at this stage",
        "script/native coverage: the mission opcode table is unmeasured",
        "exact failure/success precedence: not measured",
    ] {
        assert!(
            m01.unknowns
                .iter()
                .any(|candidate| candidate.starts_with(shared)),
            "M01's derived record keeps the shared entry {shared:?}: {:?}",
            m01.unknowns
        );
    }
    for settled in [
        "M01-B-OBJ",
        "M01-B-OPC",
        "M01-B-WAKE",
        "measured for this mission",
    ] {
        assert!(
            !m01.unknowns
                .iter()
                .any(|candidate| candidate.contains(settled)),
            "the measured results leaked into M01's record at {settled:?}: {:?}",
            m01.unknowns
        );
    }
    let committed_m01 = std::fs::read_to_string(repo_path("missions/bindings/M01.json"))
        .expect("missions/bindings/M01.json exists");
    for settled in ["M01-B-OBJ", "M01-B-OPC", "M01-B-WAKE"] {
        assert!(
            !committed_m01.contains(settled),
            "the measured results leaked into the committed M01 record at {settled:?}"
        );
    }
    // No committed sibling record carries the settled verdicts.
    for other in ["M02", "M03", "M07", "M24"] {
        let record = std::fs::read_to_string(repo_path(&format!("missions/bindings/{other}.json")))
            .unwrap_or_else(|error| panic!("missions/bindings/{other}.json reads: {error}"));
        for settled in ["M01-B-OBJ", "M01-B-OPC", "M01-B-WAKE"] {
            assert!(
                !record.contains(settled),
                "the measured results leaked into {other}'s committed record at {settled:?}"
            );
        }
    }
}

// ------------------------------------------------------- regeneration ---

/// **Regeneration harness, not an acceptance test.** Rewrites the committed
/// `missions/bindings/M01.json` from what production code derives, so the
/// record can be regenerated rather than hand-edited whenever a
/// mission-scoped entry changes (`accept_m01_a_the_committed_record_is_what_
/// the_installation_derives` byte-compares the two). Deliberately not named
/// with the `accept_m01_b_` prefix, so no task selection runs it; it is run
/// by hand, with `CS_GAME_DIR` set and `CS_REGENERATE_M01_BINDING=1`, and
/// leaves the file byte-identical when the record is already current.
#[test]
#[ignore = "regeneration harness: needs CS_GAME_DIR and CS_REGENERATE_M01_BINDING=1"]
fn regenerate_m01_b_writes_the_committed_record_through_production() {
    assert_eq!(
        std::env::var("CS_REGENERATE_M01_BINDING").as_deref(),
        Ok("1"),
        "set CS_REGENERATE_M01_BINDING=1 to rewrite missions/bindings/M01.json from the \
         production derivation"
    );
    let binding = binding_of("M01");
    binding
        .validate()
        .expect("the derived record is internally consistent");
    assert!(
        binding.unresolved_critical().is_empty(),
        "refusing to write a record with unresolved critical dependencies"
    );
    let path = repo_path("missions/bindings/M01.json");
    std::fs::write(&path, binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
    println!("wrote {}", path.display());
}

// -------------------------------------------------------------- synthetic ---

/// **A settled checklist entry reaches only the work order it was settled
/// for.**
///
/// The retail member above pins the mechanism's effect on the two missions
/// whose data decides the question; this member pins the mechanism itself
/// over the whole declared inventory, in CI, without original data: M06's
/// three shared questions are replaced by measured statements that keep the
/// checklist prefix and cite their measurements — the overrides keyed to the
/// work order the corrected layout binds the measured archive to — every
/// other declared work order keeps the shared text, and the append-side
/// mechanism (`mission_scoped_unknowns`) is untouched. If a replacement were
/// moved into the shared checklist every mission carries, this fails; if the
/// mechanism vanished, the production functions it exercises would be gone
/// with it.
#[test]
fn accept_m01_b_a_settled_checklist_entry_reaches_only_the_work_order_it_was_settled_for() {
    let checklist = source_binding_checklist();
    assert!(
        !checklist.is_empty(),
        "every record starts from the shared checklist"
    );

    let rows = mission_scoped_checklist_overrides(&label("M06"));
    assert_eq!(
        rows.len(),
        SETTLED_QUESTIONS.len(),
        "the measured archive's work order settled exactly the three questions its \
         measurements answer: {rows:?}"
    );
    let prefixes: Vec<&str> = rows
        .iter()
        .map(|(shared, _)| shared.split(": ").next().expect("a checklist prefix"))
        .collect();
    assert_eq!(
        prefixes, SETTLED_QUESTIONS,
        "the settled questions are exactly the measured ones, in checklist order"
    );
    for (shared, measured) in rows {
        assert!(
            checklist.contains(shared),
            "a replacement must name a shared checklist entry verbatim, not {shared:?}"
        );
        let prefix = shared.split(": ").next().expect("a checklist prefix");
        assert!(
            measured.starts_with(&format!("{prefix}: ")),
            "the measured statement keeps the checklist prefix {prefix:?}: {measured}"
        );
        assert!(
            measured.contains("M01-B-"),
            "the measured statement cites the tasks that measured it: {measured}"
        );
        assert!(
            measured.contains("Residual:"),
            "the measured statement names what it did not settle: {measured}"
        );
    }

    // Over the whole declared inventory, only M06's record is touched.
    let mut others = 0usize;
    for (work_order, _) in load_inventory().iter() {
        if work_order.as_str() == "M06" {
            continue;
        }
        assert!(
            mission_scoped_checklist_overrides(work_order).is_empty(),
            "the measurements settled on the `zbd/c1c/m01` archive reached {}'s record",
            work_order.as_str()
        );
        others += 1;
    }
    assert_eq!(
        others, 23,
        "the declared inventory carries the other 23 work orders"
    );

    // The append-side mechanism is untouched: M12 still carries its
    // passenger-identity limitation (measured on `zbd/c2/m01`, the archive the
    // corrected layout binds to M12), and M01's record gained no appended
    // limitation and no overrides — its archive was never measured.
    assert!(
        mission_scoped_unknowns(&label("M12"))
            .iter()
            .any(|entry| entry.contains("passenger identity")),
        "M12's mission-scoped limitation is untouched by the override mechanism"
    );
    assert!(
        mission_scoped_unknowns(&label("M01")).is_empty()
            && mission_scoped_checklist_overrides(&label("M01")).is_empty(),
        "M01's record carries neither overrides nor appended limitations — its archive was \
         never measured"
    );
}
