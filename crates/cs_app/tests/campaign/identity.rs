//! Acceptance stage F50-A: stable typed identity and the admission rules
//! that keep a mission binding honest
//! (`specs/F50-per-mission-compatibility-and-full-campaign-closure.md`,
//! section `### F50-A`, and `docs/contracts/IDENTITY-CONTENT.md`).
//!
//! Everything here runs production code in
//! `cs_content::campaign_bindings`.

use cs_content::campaign_bindings::{
    BindingCategory, BindingError, BindingRow, CampaignBindings, CategoryState, LabelError,
    MissionLabel, Progression, SubsystemId, WORLD_ROLE,
};
use cs_types::content::ContentKind;

use crate::common::{cid, claim, designed, label, load_inventory, ready_binding};

/// A declared mission is a discovery label with no retail identity, and its
/// placeholder can be bound exactly once: the second binding is a duplicate
/// identity, not an update.
#[test]
fn accept_f50_a_a_discovery_label_never_stands_in_for_a_retail_identity() {
    let inventory = load_inventory();
    let mut campaign = CampaignBindings::from_inventory(&inventory)
        .expect("the declared inventory builds placeholders");
    let m01 = label("M01");

    let placeholder = campaign.get(&m01).expect("M01 is declared");
    assert!(placeholder.is_placeholder());
    assert_eq!(
        placeholder.discovery_title.as_deref(),
        Some("The Lost Treasure"),
        "the discovery title rides along, outside identity"
    );
    assert!(
        placeholder.catalog_identity().is_none(),
        "a work-order label is not a catalog identity"
    );
    assert!(matches!(
        placeholder.progression,
        Progression::Unknown { .. }
    ));

    campaign
        .bind(ready_binding(
            "M01",
            "The Lost Treasure",
            Progression::known(vec![label("M02")]),
        ))
        .expect("the placeholder binds once");
    let bound = campaign.get(&m01).expect("M01 is recorded");
    assert!(!bound.is_placeholder());
    let identity = bound
        .catalog_identity()
        .expect("the mission identity row is bound");
    assert_eq!(identity.kind(), ContentKind::Mission);
    assert_eq!(identity.key(), "m01-mission");
    assert!(matches!(bound.progression, Progression::Known { .. }));

    let error = campaign
        .bind(ready_binding(
            "M01",
            "A contradictory second binding",
            Progression::known(Vec::new()),
        ))
        .expect_err("a bound mission is never replaced");
    assert!(
        matches!(error, BindingError::AlreadyBound { .. }),
        "{error}"
    );

    // Binding one mission leaves the rest of the denominator unresolved.
    let m02 = label("M02");
    assert!(
        campaign
            .get(&m02)
            .expect("M02 is declared")
            .is_placeholder(),
        "binding M01 must not touch M02"
    );

    // Lowercase spelling is a different string with the same meaning: it is
    // refused rather than kept as a second identity.
    assert!(matches!(
        MissionLabel::new("m01"),
        Err(LabelError::BadCharacter { ch: 'm' })
    ));
}

/// Records that break an identity or completeness rule are refused when they
/// are inserted, with the offending identity in the error.
#[test]
fn accept_f50_a_invalid_records_are_refused_on_admission() {
    let mut campaign = CampaignBindings::new();

    // An identity row must carry the kind its role means.
    let mut wrong_kind = ready_binding("M01", "One", Progression::known(Vec::new()));
    {
        let Some(CategoryState::Rows(rows)) = wrong_kind
            .categories
            .get_mut(&BindingCategory::MissionIdentity)
        else {
            panic!("the fixture records its identity as rows");
        };
        rows.retain(|row| row.role() != WORLD_ROLE);
        rows.push(
            BindingRow::content(
                WORLD_ROLE,
                cid(ContentKind::Mission, "wrong-world"),
                designed("f50.a.synthetic.wrong_kind"),
            )
            .expect("the row is well formed"),
        );
    }
    let error = campaign
        .insert(wrong_kind)
        .expect_err("a world row pointing at a mission is refused");
    assert!(
        matches!(
            &error,
            BindingError::WrongKind {
                role,
                expected: ContentKind::World,
                found: ContentKind::Mission,
            } if role == WORLD_ROLE
        ),
        "{error}"
    );

    // Every required subsystem needs a row: an empty dependency list can
    // never read as "nothing left to do".
    let mut without_dependencies = ready_binding("M02", "Two", Progression::known(Vec::new()));
    without_dependencies.dependencies.clear();
    let error = campaign
        .insert(without_dependencies)
        .expect_err("a mission without subsystem rows is refused");
    assert!(
        matches!(&error, BindingError::MissingSubsystem { subsystem } if subsystem.as_str() == "F18"),
        "{error}"
    );

    // A subsystem outside the required set is a dangling identity.
    let mut unknown_subsystem = ready_binding("M03", "Three", Progression::known(Vec::new()));
    let mut extra = unknown_subsystem.dependencies[0].clone();
    extra.subsystem = SubsystemId::new("F99").expect("F99 is a well formed identity");
    unknown_subsystem.dependencies.push(extra);
    let error = campaign
        .insert(unknown_subsystem)
        .expect_err("an unknown subsystem is refused");
    assert!(
        matches!(&error, BindingError::UnknownSubsystem { subsystem } if subsystem.as_str() == "F99"),
        "{error}"
    );

    // The same subsystem row twice is a duplicate identity.
    let mut duplicated = ready_binding("M04", "Four", Progression::known(Vec::new()));
    let first = duplicated.dependencies[0].clone();
    duplicated.dependencies.push(first);
    let error = campaign
        .insert(duplicated)
        .expect_err("a duplicated subsystem row is refused");
    assert!(
        matches!(
            &error,
            BindingError::DuplicateSubsystem { subsystem } if subsystem.as_str() == "F18"
        ),
        "{error}"
    );

    // An empty row list is not a category: it is refused rather than
    // counted as a category with no children.
    assert!(matches!(
        CategoryState::rows(Vec::new()),
        Err(BindingError::EmptyRows)
    ));
    let mut empty_category = ready_binding("M05", "Five", Progression::known(Vec::new()));
    empty_category.categories.insert(
        BindingCategory::Interactions,
        CategoryState::Rows(Vec::new()),
    );
    let error = campaign
        .insert(empty_category)
        .expect_err("a category with no rows is refused");
    assert!(
        matches!(
            &error,
            BindingError::EmptyCategory {
                category: BindingCategory::Interactions
            }
        ),
        "{error}"
    );

    // A blank reason is not an explicit unknown.
    assert!(matches!(
        Progression::unknown(claim("f50.a.blank_progression"), "   "),
        Err(BindingError::EmptyReason { .. })
    ));
    assert!(matches!(
        BindingRow::unknown("objective", claim("f50.a.blank_row"), ""),
        Err(BindingError::Resolved(_))
    ));
}
