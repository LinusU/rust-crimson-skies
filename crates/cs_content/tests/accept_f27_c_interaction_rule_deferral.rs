//! Acceptance scenario F27-C: the interaction-rule schema reports which
//! declared options a session actually applies and which are deferred.
//!
//! Spec: `specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`,
//! stage `### F27-C`. Task test prefix: `accept_f27_c_`.
//! Decision record:
//! `docs/findings/2026-10-02-f27-c-candidate-filtering-and-hit-damage-routing.md`.
//!
//! The acceptance criterion this pins: `penetration`, `ricochet` and
//! `ammo_switching` are **not** consumed by any production code, and that is
//! an explicit deferral to F27-D with the reason recorded — visible in the
//! schema itself, not only in a findings file. These tests drive
//! [`cs_content::weapons`]'s production reporting, so removing the report (or
//! wrongly marking a deferred option as applied) fails them.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data. No `CS_GAME_DIR` access.

use cs_content::weapons::{
    APPLIED_BY_CANDIDATE_FILTER, DEFERRAL_STAGE, DeclaredSelfHitRule, InteractionOption,
    InteractionRules, synthetic_interaction_rules,
};
use cs_types::content::{Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;

fn claim() -> ClaimId {
    ClaimId::new("f27c.deferral-test").expect("a valid claim id")
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, Provenance::designed(claim())))
}

/// The three options this stage defers: no production code reads them, and
/// each one is reported as deferred to F27-D with a stated reason.
#[test]
fn accept_f27_c_penetration_ricochet_and_ammo_switching_are_deferred_to_f27_d() {
    let rules = synthetic_interaction_rules();

    for option in [
        InteractionOption::Penetration,
        InteractionOption::Ricochet,
        InteractionOption::AmmoSwitching,
    ] {
        assert_eq!(
            rules.applied_by(option),
            None,
            "{} is read by no production code, so the schema must not claim \
             otherwise",
            option.label()
        );
        let (stage, reason) = option
            .deferred_to()
            .unwrap_or_else(|| panic!("{} is deferred and must say so", option.label()));
        assert_eq!(
            stage,
            DEFERRAL_STAGE,
            "{} is deferred to the stage that audits the original ammunition",
            option.label()
        );
        assert!(
            !reason.is_empty(),
            "{}'s deferral must carry a reason, not just a stage",
            option.label()
        );
    }

    // And the same three come back from the aggregate report, in
    // `InteractionOption::ALL` order.
    let deferred = rules.deferred();
    assert_eq!(
        deferred
            .iter()
            .map(|(option, _, _)| *option)
            .collect::<Vec<_>>(),
        vec![
            InteractionOption::Penetration,
            InteractionOption::Ricochet,
            InteractionOption::AmmoSwitching,
        ],
        "the schema's own report names exactly the three deferred options"
    );
    for (_, stage, reason) in &deferred {
        assert_eq!(*stage, DEFERRAL_STAGE);
        assert!(!reason.is_empty());
    }
}

/// The other two options **are** applied, by name: the F27-C candidate query
/// is the production path that consults them, so the schema says where.
#[test]
fn accept_f27_c_self_hit_and_friendly_fire_are_applied_by_the_candidate_query() {
    let rules = synthetic_interaction_rules();

    for option in [InteractionOption::SelfHit, InteractionOption::FriendlyFire] {
        assert_eq!(
            rules.applied_by(option),
            Some(APPLIED_BY_CANDIDATE_FILTER),
            "{} is consulted by the F27-C candidate query",
            option.label()
        );
        assert_eq!(
            option.deferred_to(),
            None,
            "{} has no outstanding deferral: it is applied",
            option.label()
        );
    }

    assert!(
        rules.deferred().len() < InteractionOption::ALL.len(),
        "the two applied options are absent from the deferral report"
    );
}

/// "Applied" and "known" are separate questions and the schema reports them
/// separately: a production path consults the self-hit rule whether or not
/// this record knows what it is, and an unknown value refuses to lower. A
/// test that conflated the two would let a record claim a known self-hit rule
/// it never declared.
#[test]
fn accept_f27_c_applied_and_known_are_reported_separately() {
    let rules = synthetic_interaction_rules();
    for option in InteractionOption::ALL {
        assert!(
            rules.is_known(*option),
            "the synthetic fixture declares every option with designed provenance"
        );
    }

    let unknown = InteractionRules {
        self_hit: Resolved::unknown(claim(), "the original self-hit rule is unmeasured")
            .expect("a nonempty reason"),
        ..synthetic_interaction_rules()
    };
    assert!(
        !unknown.is_known(InteractionOption::SelfHit),
        "an explicit unknown is not a known value: it must never be read as one"
    );
    assert!(
        unknown.is_known(InteractionOption::FriendlyFire),
        "one unknown option does not make the whole record unknown"
    );
    assert_eq!(
        unknown.applied_by(InteractionOption::SelfHit),
        Some(APPLIED_BY_CANDIDATE_FILTER),
        "the production path still consults the option; it is the lowering \
         boundary that refuses the unknown value, which is a different question"
    );

    // Widening the declared self-hit rule is visible through the same query.
    let allowed = InteractionRules {
        self_hit: known(DeclaredSelfHitRule::Allowed),
        ..synthetic_interaction_rules()
    };
    assert!(allowed.is_known(InteractionOption::SelfHit));
    assert_eq!(
        allowed.deferred().len(),
        3,
        "changing a rule's value does not change which options are applied or deferred"
    );
}

/// Every declared option is covered by the report: adding a sixth option to
/// `InteractionOption::ALL` without deciding whether it is applied or deferred
/// would make the schema silent about it, and this fails.
#[test]
fn accept_f27_c_every_declared_option_is_either_applied_or_deferred() {
    let rules = synthetic_interaction_rules();
    let applied = InteractionOption::ALL
        .iter()
        .filter(|option| rules.applied_by(**option).is_some())
        .count();
    let deferred = rules.deferred().len();
    assert_eq!(
        applied + deferred,
        InteractionOption::ALL.len(),
        "every declared option is reported exactly once, so none can be silently \
         unaccounted for"
    );
    assert_eq!(applied, 2);
    assert_eq!(deferred, 3);
}
