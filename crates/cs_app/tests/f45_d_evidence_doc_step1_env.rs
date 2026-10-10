//! The F45-D evidence harness's module doc must document an acceptance run
//! that really produces the artifacts the harness reads (task #1156).
//!
//! The retail acceptance test derives `front-end-screens.json` and the GPU
//! acceptance tests write their `f45-d-*.png` captures only while
//! `CS_EVIDENCE_DIR` is set — that is, while step 1 of the documented
//! sequence runs; the harness of step 2 only reads, hashes and re-checks
//! what step 1 left behind. A step 1 without that prefix therefore leaves
//! the evidence directory holding nothing but the log, and step 2 fails with
//! "cannot read …/front-end-screens.json: No such file or directory" while
//! the harness's own panic text says step 1 must set `CS_EVIDENCE_DIR`.
//!
//! The doc itself is this task's deliverable, so this test embeds the
//! shipped harness file (`include_str!`) and asserts on the documented
//! sequence itself, the way `cs_assets/tests/accept_f02_doc_module_doc.rs`
//! guards its doc: deleting `CS_EVIDENCE_DIR=…` from step 1, or letting the
//! steps disagree on the directory, fails here.

/// The shipped harness file, embedded at compile time so the test reads the
/// real module doc rather than a copy.
const EVIDENCE_RS: &str = include_str!("ui/evidence.rs");

/// The directory the sequence must use, named by `docs/contracts/CLI-EVIDENCE.md`
/// as `private/evidence/<TASK-ID>/`.
const EVIDENCE_DIR: &str = "private/evidence/F45-D";

/// The documented command blocks (each ```` ```sh ```` fenced block of the
/// module doc), in order, as their lines with the `//!` prefix removed.
fn command_blocks(source: &str) -> Vec<Vec<String>> {
    let mut blocks: Vec<Vec<String>> = Vec::new();
    let mut current: Option<Vec<String>> = None;
    for line in source.lines() {
        let Some(doc) = line.strip_prefix("//!") else {
            break;
        };
        let doc = doc.strip_prefix(' ').unwrap_or(doc);
        match &mut current {
            Some(block) => {
                if doc.trim() == "```" {
                    blocks.push(current.take().expect("the open block is closed"));
                } else {
                    block.push(doc.to_owned());
                }
            }
            None if doc.trim_end().ends_with("```sh") => current = Some(Vec::new()),
            None => {}
        }
    }
    assert!(current.is_none(), "the last command block is closed");
    blocks
}

/// The value the block's `CS_EVIDENCE_DIR=…` prefix line holds, if it has one.
fn evidence_dir(block: &[String]) -> Option<String> {
    block.iter().find_map(|line| {
        let value = line.trim_start().strip_prefix("CS_EVIDENCE_DIR=")?;
        Some(value.trim_end().trim_end_matches('\\').trim().to_owned())
    })
}

/// Step 1, executed verbatim from a clean evidence directory, must produce
/// the artifacts step 2 reads: the acceptance run itself carries
/// `CS_EVIDENCE_DIR`, and both steps name the same directory.
#[test]
fn f45_d_evidence_doc_step1_env_the_documented_acceptance_run_sets_cs_evidence_dir() {
    let blocks = command_blocks(EVIDENCE_RS);
    assert_eq!(
        blocks.len(),
        3,
        "the module doc documents three command steps (the run, the harness, the validator)"
    );
    let [step1, step2, _step3] = &blocks[..] else {
        unreachable!("three blocks were just counted")
    };

    let step1_joined = step1.join("\n");
    let assignment = evidence_dir(step1).unwrap_or_else(|| {
        panic!(
            "step 1 must prefix the acceptance run with CS_EVIDENCE_DIR={EVIDENCE_DIR}: the \
             retail inventory and the GPU captures are written only while it is set, so without \
             it step 2 cannot read them\n\nstep 1 as documented:\n{step1_joined}"
        )
    });
    assert_eq!(
        assignment, EVIDENCE_DIR,
        "step 1 must write into the task's evidence directory"
    );

    let cargo = step1_joined
        .find("cargo test --workspace --locked -- accept_f45_d_ --include-ignored")
        .expect("step 1 runs the acceptance selection");
    assert!(
        step1_joined
            .find("CS_EVIDENCE_DIR=")
            .expect("the prefix was found above")
            < cargo,
        "CS_EVIDENCE_DIR must prefix the acceptance run, not trail it"
    );
    assert!(
        step1_joined.contains(&format!("tee {EVIDENCE_DIR}/cargo-test.log")),
        "the log must land in the same directory as the artifacts the harness reads"
    );

    assert_eq!(
        evidence_dir(step2).as_deref(),
        Some(EVIDENCE_DIR),
        "step 2 must read the directory step 1 wrote"
    );
    assert!(
        step2
            .join("\n")
            .contains("cargo test --locked -p cs_app --test ui -- evidence_report_f45_d --ignored"),
        "step 2 runs the harness this test guards"
    );
    assert!(
        EVIDENCE_RS.contains("step 1 must set CS_EVIDENCE_DIR"),
        "the harness's own failure message must keep telling the runner what the doc says"
    );
}
