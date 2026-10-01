use cs_app::ui::front_end::{
    Action, ActionSource, Guard, RequestKind, Row, Screen, TABLE, TableProblem, validate_rows,
    validate_table,
};

#[test]
fn accept_f45_a_table_is_total_and_every_screen_can_escape_and_reach_the_menu() {
    assert_eq!(validate_table(), Vec::new());
    for screen in Screen::ALL {
        assert!(
            TABLE.iter().any(|row| row.from == screen),
            "{screen:?} has no row"
        );
    }
}

#[test]
fn accept_f45_a_validator_reports_each_way_a_table_can_be_broken() {
    // Duplicate (from, action).
    let mut duplicated = TABLE.to_vec();
    duplicated.push(TABLE[0]);
    assert!(
        validate_rows(&duplicated).contains(&TableProblem::Duplicate {
            from: TABLE[0].from,
            action: TABLE[0].action,
        })
    );

    // Without the Recon exit the screen traps the player.
    let trapped: Vec<Row> = TABLE
        .iter()
        .copied()
        .filter(|row| !(row.from == Screen::Recon && row.action == Action::Back))
        .collect();
    let problems = validate_rows(&trapped);
    assert!(problems.contains(&TableProblem::NoEscape {
        screen: Screen::Recon
    }));
    assert!(problems.contains(&TableProblem::NoPathToMenu {
        screen: Screen::Recon
    }));

    // Without the only way in, a screen is unreachable.
    let orphaned: Vec<Row> = TABLE
        .iter()
        .copied()
        .filter(|row| row.to != Screen::Scrapbook)
        .collect();
    assert!(
        validate_rows(&orphaned).contains(&TableProblem::Unreachable {
            screen: Screen::Scrapbook
        })
    );
}

#[test]
fn accept_f45_a_action_keys_are_unique_and_round_trip() {
    for action in Action::ALL {
        assert_eq!(Action::from_key(action.key()), Some(action));
    }
    assert_eq!(Action::from_key("no-such-button"), None);
    let mut keys: Vec<_> = Action::ALL.iter().map(|a| a.key()).collect();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), Action::ALL.len());
}

#[test]
fn accept_f45_a_only_a_user_action_can_ask_to_discard_a_draft() {
    // A discard prompt answers a button; an application result cannot raise one.
    for row in TABLE {
        if row.guard == Guard::DiscardsDraft {
            assert_eq!(row.action.source(), ActionSource::User, "{row:?}");
            assert_eq!(row.request, RequestKind::None, "{row:?}");
        }
    }
}

#[test]
fn accept_f45_a_every_action_in_the_vocabulary_is_used_by_some_row() {
    for action in Action::ALL {
        assert!(
            TABLE.iter().any(|row| row.action == action),
            "{action:?} is in no row"
        );
    }
}
