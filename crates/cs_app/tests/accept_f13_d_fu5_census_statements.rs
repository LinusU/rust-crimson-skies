//! Acceptance scenario F13-D-FU5: records written before F13-D must not state
//! the pre-F13-D census (every located program stops at `unknown_opcode`, the
//! bytecode mission VM unlocated) as a current fact (task #1422).
//!
//! Task test prefix: `accept_f13_d_fu5_`. The statements are prose, so the
//! tests read the four records through `include_str!` and pin the dated F13-D
//! correction; removing the correction fails them.

const MISSION_CONTROL_RS: &str = include_str!("../src/mission_control.rs");
const EVIDENCE_M01_LC_RS: &str = include_str!("evidence_report_m01_lc.rs");
const EVIDENCE_F38_B_RS: &str = include_str!("../../cs_script/tests/evidence_report_f38_b.rs");
const OPC_FINDING_MD: &str = include_str!(
    "../../../docs/findings/scripts/2026-10-12-m01-b-opc-directive-programs-in-the-f13-c-probe.md"
);

#[test]
fn accept_f13_d_fu5_module_doc_carries_the_f13_d_correction() {
    assert!(MISSION_CONTROL_RS.contains("Correction, 2026-10-14, F13-D #640"));
    assert!(MISSION_CONTROL_RS.contains("0 stopped at `unknown_opcode`"));
}

#[test]
fn accept_f13_d_fu5_finding_carries_a_dated_correction_banner() {
    assert!(OPC_FINDING_MD.contains("**Correction (2026-10-14, F13-D task #640"));
    assert!(OPC_FINDING_MD.contains("**0** stopped at `unknown_opcode`"));
}

#[test]
fn accept_f13_d_fu5_evidence_method_text_no_longer_claims_the_old_census() {
    for (path, source) in [
        ("evidence_report_m01_lc.rs", EVIDENCE_M01_LC_RS),
        ("evidence_report_f38_b.rs", EVIDENCE_F38_B_RS),
    ] {
        assert!(
            source.contains("F13-D (#640)") || source.contains("F13-D (#640, owner"),
            "{path} must cite F13-D's result"
        );
        assert!(
            !source.contains("F13-D has not run") && !source.contains("remains unlocated"),
            "{path} still states the pre-F13-D census as current"
        );
    }
}
