//! Acceptance scenario F32-R: the seven combat roles are designed project
//! vocabulary, not the original's AI role set.
//!
//! Spec: `specs/F32-ai-combat-formations-aces-and-difficulty.md`, section
//! `### F32-D` (research boundary). Task test prefix: `accept_f32_r_`.
//!
//! The retail search (data members and executable image) lives with the
//! measurement in `cs_content::ai`; this file checks the runtime side: no role
//! claims an original counterpart, and the role vocabulary still has the seven
//! labels the declared side mirrors.

use cs_sim::ai::combat::{CombatRole, ORIGINAL_AI_ROLE_SET_MEASURED};

#[test]
fn accept_f32_r_no_runtime_role_claims_an_original_counterpart() {
    assert!(!std::hint::black_box(ORIGINAL_AI_ROLE_SET_MEASURED));
    assert_eq!(CombatRole::ALL.len(), 7);
    for role in CombatRole::ALL {
        assert!(
            !role.original_counterpart_measured(),
            "{role}: the original's role set is unmeasured"
        );
    }
}

#[test]
fn accept_f32_r_role_labels_are_unique_and_stable() {
    let mut labels: Vec<&str> = CombatRole::ALL.iter().map(|r| r.label()).collect();
    labels.sort_unstable();
    labels.dedup();
    assert_eq!(labels.len(), 7);
}
