//! Acceptance stage F50-A: the declared campaign inventory — the frozen
//! denominator (`specs/F50-per-mission-compatibility-and-full-campaign-closure.md`,
//! section `### F50-A`, and the owner ruling of 2026-09-28: "The denominator
//! is fixed from the supported original inventory and cannot shrink").
//!
//! The reader under test is
//! `cs_content::campaign_bindings::CampaignInventory`, production code.

use cs_content::campaign_bindings::{
    CampaignInventory, InventoryError, InventoryLineError, MissionLabel,
};

use crate::common::{load_inventory, repo_path};

/// The work orders the owner-authored `missions/README.md` lists, in file
/// order. That file is a protected path: an agent cannot quietly edit the
/// list to match a shortened inventory.
fn work_orders() -> Vec<(MissionLabel, String)> {
    let text = std::fs::read_to_string(repo_path("missions/README.md"))
        .expect("the owner-authored work-order list reads");
    let mut orders = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("- [") else {
            continue;
        };
        let Some((head, tail)) = rest.split_once("](") else {
            continue;
        };
        if !tail.contains(".md)") {
            continue;
        }
        let Some((id, title)) = head.split_once(": ") else {
            continue;
        };
        let Ok(label) = MissionLabel::new(id) else {
            continue;
        };
        orders.push((label, title.to_owned()));
    }
    orders
}

/// The denominator is the owner's inventory, read as data: every work order
/// in `missions/README.md` is declared exactly once, with the same
/// discovery label and title, and nothing else is.
#[test]
fn accept_f50_a_denominator_matches_the_owner_work_orders() {
    let inventory = load_inventory();
    let orders = work_orders();

    assert_eq!(
        orders.len(),
        24,
        "the pack ships 24 mission work orders; a different count here means the list changed"
    );
    assert_eq!(
        inventory.len(),
        orders.len(),
        "the declared denominator covers every work order and no others"
    );
    for ((declared_label, declared_title), (order_label, order_title)) in
        inventory.iter().zip(orders.iter())
    {
        assert_eq!(
            declared_label, order_label,
            "the denominator declares {declared_label} where the work-order list says {order_label}"
        );
        assert_eq!(
            declared_title, order_title,
            "the discovery title of {declared_label} matches the work-order list"
        );
    }
}

/// A line that is not exactly `label<TAB>title`, a blank title, a
/// malformed label or a repeated label fails the whole file instead of
/// dropping a mission from the denominator, and an inventory that declares
/// nothing is refused outright.
#[test]
fn accept_f50_a_a_malformed_inventory_never_drops_a_mission_silently() {
    assert!(matches!(
        CampaignInventory::parse(""),
        Err(InventoryError::Empty)
    ));
    assert!(matches!(
        CampaignInventory::parse("# comments only\n\n"),
        Err(InventoryError::Empty)
    ));
    assert!(matches!(
        CampaignInventory::parse("M01\tA Mission\n"),
        Ok(inventory) if inventory.len() == 1
    ));
    // Windows line endings are the same inventory, not a different one.
    assert!(matches!(
        CampaignInventory::parse("M01\tFirst\r\nM02\tSecond\r\n"),
        Ok(inventory) if inventory.len() == 2
    ));

    assert!(matches!(
        CampaignInventory::parse("M01\n"),
        Err(InventoryError::Line {
            line: 1,
            kind: InventoryLineError::FieldCount { found: 1 },
        })
    ));
    assert!(matches!(
        CampaignInventory::parse("M01\tFirst\nM02\tSecond\nM03\n"),
        Err(InventoryError::Line {
            line: 3,
            kind: InventoryLineError::FieldCount { found: 1 },
        })
    ));
    assert!(matches!(
        CampaignInventory::parse("M01\t\n"),
        Err(InventoryError::Line {
            line: 1,
            kind: InventoryLineError::EmptyTitle,
        })
    ));
    assert!(matches!(
        CampaignInventory::parse("m01\tA Mission\n"),
        Err(InventoryError::Line {
            line: 1,
            kind: InventoryLineError::Label(_),
        })
    ));
    assert!(matches!(
        CampaignInventory::parse("M01\tFirst\nM01\tSecond\n"),
        Err(InventoryError::Line {
            line: 2,
            kind: InventoryLineError::DuplicateLabel { .. },
        })
    ));

    // A file that cannot be read is an error, never an empty denominator.
    let missing = repo_path("missions/bindings/does-not-exist.tsv");
    assert!(matches!(
        CampaignInventory::load(&missing),
        Err(InventoryError::Io { .. })
    ));
}
