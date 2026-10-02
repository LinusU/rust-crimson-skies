//! Acceptance scenario F30-B: the declared selection-action table — the
//! provenance-carrying record an IA preset or control scheme produces and
//! the runtime binding lowers from.
//!
//! Spec: `specs/F30-targeting-classification-aim-assistance-and-threat-cues.md`,
//! stage `### F30-B`. Task test prefix: `accept_f30_b_`.
//!
//! These tests drive production code only:
//! [`cs_content::target_rules`]'s [`DeclaredSelectionActions`],
//! [`DeclaredSelectionAction`], [`DeclaredAction`] and
//! [`SelectionActionsError`]. The table is a schema, so what is under test
//! is what it *refuses* to record: a target action on a continuous axis, two
//! actions on one command edge, and an action the importer could not
//! evidence — which the table carries as an explicit unknown rather than
//! dropping or defaulting.
//!
//! Every value here is newly authored synthetic fixture data, never
//! original game data.

use cs_content::target_rules::{
    DeclaredAction, DeclaredSelectionAction, DeclaredSelectionActions, SelectionActionsError,
    declared_synthetic_selection_actions,
};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::evidence::ClaimStatus;
use cs_types::input::FlightCommand;

fn claim() -> ClaimId {
    ClaimId::new("f30b.action-table-test").expect("a valid claim id")
}

fn subject() -> ContentId {
    ContentId::from_source(ContentKind::IaScenario, "synthetic.target-range")
        .expect("a valid subject id")
}

fn binding(command: FlightCommand, action: Resolved<DeclaredAction>) -> DeclaredSelectionAction {
    DeclaredSelectionAction {
        command,
        action,
        evidence: Provenance::designed(claim()),
    }
}

fn known(action: DeclaredAction) -> Resolved<DeclaredAction> {
    Resolved::Known(Known::new(action, Provenance::designed(claim())))
}

/// The action a resolved record carries, ignoring *which* claim backs it.
fn action_of(resolved: &Resolved<DeclaredAction>) -> Option<DeclaredAction> {
    match resolved {
        Resolved::Known(known) => Some(known.value),
        Resolved::Unknown { .. } => None,
    }
}

/// The declared vocabulary is total and self-describing: every action has a
/// label, every label round-trips, and the fixture table binds the two cycle
/// edges plus one nearest-attacker binding under its own subject and
/// synthetic origin.
#[test]
fn accept_f30_b_declared_action_table_is_total_and_addressable() {
    for action in DeclaredAction::ALL {
        assert_eq!(
            DeclaredAction::from_label(action.label()),
            Some(*action),
            "every declared action's label round-trips"
        );
    }
    assert_eq!(
        DeclaredAction::from_label("not_an_action"),
        None,
        "an unknown label is not an action"
    );

    let table = declared_synthetic_selection_actions();
    assert_eq!(table.subject(), &subject());
    assert_eq!(*table.origin(), Origin::SyntheticFixture);
    assert_eq!(table.bindings().len(), 3);
    assert_eq!(
        table
            .binding(FlightCommand::TargetNext)
            .and_then(|declared| action_of(&declared.action)),
        Some(DeclaredAction::NextHostile),
        "the next edge declares the next-hostile action"
    );
    assert_eq!(
        table
            .binding(FlightCommand::CycleWeapon)
            .and_then(|declared| action_of(&declared.action)),
        Some(DeclaredAction::NearestAttacker),
        "a binding is found by its command edge, wherever the table ordered it"
    );
    assert!(
        table.binding(FlightCommand::TargetPrev).is_some(),
        "the previous edge is declared too"
    );
    assert!(
        table.binding(FlightCommand::FirePrimary).is_none(),
        "a command the preset does not bind has no record — it is not a \\
         target command, which is a different statement from an unevidenced one"
    );
    for declared in table.bindings() {
        assert!(
            !declared.command.is_continuous(),
            "no declared target action is bound to an axis"
        );
        assert_eq!(
            declared.evidence.class,
            ClaimStatus::Designed,
            "the synthetic preset is designed content, never a retail claim"
        );
    }
}

/// The table refuses the two structural mistakes a mapping can make: a
/// target action on a continuous axis (it would fire every frame the axis
/// moved) and two actions on one command edge (one edge runs one action).
#[test]
fn accept_f30_b_declared_action_table_refuses_axis_and_duplicate_bindings() {
    assert_eq!(
        DeclaredSelectionActions::try_new(
            subject(),
            Origin::SyntheticFixture,
            vec![binding(
                FlightCommand::Pitch,
                known(DeclaredAction::NextHostile)
            )],
            Provenance::designed(claim()),
        ),
        Err(SelectionActionsError::ContinuousCommand {
            command: FlightCommand::Pitch,
        })
    );
    assert_eq!(
        DeclaredSelectionActions::try_new(
            subject(),
            Origin::SyntheticFixture,
            vec![
                binding(
                    FlightCommand::TargetNext,
                    known(DeclaredAction::NextHostile)
                ),
                binding(
                    FlightCommand::TargetNext,
                    known(DeclaredAction::NearestHostile)
                ),
            ],
            Provenance::designed(claim()),
        ),
        Err(SelectionActionsError::DuplicateCommand {
            command: FlightCommand::TargetNext,
        })
    );
    // The same action on two different edges is legal: two keys may cycle.
    assert!(
        DeclaredSelectionActions::try_new(
            subject(),
            Origin::SyntheticFixture,
            vec![
                binding(
                    FlightCommand::TargetNext,
                    known(DeclaredAction::NextHostile)
                ),
                binding(
                    FlightCommand::CycleWeapon,
                    known(DeclaredAction::NextHostile)
                ),
            ],
            Provenance::designed(claim()),
        )
        .is_ok()
    );
}

/// An action the importer could not evidence is carried as an explicit
/// unknown with its claim and reason, so the boundary can refuse it by name
/// instead of the table silently presenting an inert key.
#[test]
fn accept_f30_b_unevidenced_action_is_carried_as_an_explicit_unknown() {
    let unknown = Resolved::unknown(claim(), "the original key binding is unmeasured")
        .expect("a reason was given");
    let table = DeclaredSelectionActions::try_new(
        subject(),
        Origin::SyntheticFixture,
        vec![binding(FlightCommand::TargetNext, unknown.clone())],
        Provenance::designed(claim()),
    )
    .expect("an unresolved action is still a structurally valid table");
    let declared = table
        .binding(FlightCommand::TargetNext)
        .expect("the binding is recorded");
    assert_eq!(
        declared.action, unknown,
        "the unknown is preserved verbatim"
    );
    assert_eq!(
        table.bindings().len(),
        1,
        "it is not dropped from the table"
    );
}
