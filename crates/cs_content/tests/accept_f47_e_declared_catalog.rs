//! Acceptance stage F47-C2: the declared original scrapbook catalog
//! (`specs/F47-scrapbook-records-mementos-and-mission-replay.md`, the `### F47-D`
//! audit's follow-up), shared contract `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! [`cs_content::scrapbook::declare`] turns the discovered table into the
//! runtime [`ScrapbookCatalog`] — one [`ScrapbookEntry`] per
//! [`DiscoveredItem`], keyed by `install_file_key(entry_key)` — without
//! inventing anything the bytes do not state:
//!
//! * the **synthetic** tests hand [`declare`] an authored
//!   [`DiscoveredScrapbook`]: id keying and order, the title join through the
//!   resource-name resolver, the absent picture state, the explicit unknown
//!   unlock that never unlocks, and the refusal of a record the schema does
//!   not cover. They run in CI.
//! * the **retail** test (`#[ignore = "requires CS_GAME_DIR"]`) runs the
//!   production chain — [`DiscoveredScrapbook::discover`],
//!   [`cs_content::scrapbook::resource_names`], [`declare`], then
//!   [`cs_content::scrapbook::audit`] — over the owner's installation and pins
//!   the measured numbers. Without `CS_GAME_DIR` it fails loudly rather than
//!   passing vacuously.

use std::collections::BTreeMap;
use std::path::Path;

use cs_content::catalog::Catalog;
use cs_content::catalog::baseline::{install_file_key, retail_baseline};
use cs_content::scrapbook::{
    DeclareError, DiscoveredItem, DiscoveredPage, DiscoveredScrapbook, EntryKind, EntryVisibility,
    UnlockFact, audit, declare, resource_names,
};
use cs_types::asset_id::SourceSpan;
use cs_types::content::ContentKind;
use cs_types::evidence::ContentHash;

// ------------------------------------------------------------- fixtures ---

/// One discovered item: `key` on `page`, `TitleResID` `title`, capture member
/// `capture` (a picture the installation holds) or `None`.
fn item(key: &str, page: u32, title: &str, capture: Option<&str>, line: u64) -> DiscoveredItem {
    DiscoveredItem {
        key: key.to_owned(),
        id_key: Some(install_file_key(key)),
        page,
        spread: 0,
        slot: 0,
        objective: Some(0),
        image: format!("{key}.BMP"),
        artwork: capture.iter().map(|member| (*member).to_owned()).collect(),
        capture: capture.map(str::to_owned),
        title: title.to_owned(),
        text: "0".to_owned(),
        line,
    }
}

/// A discovered scrapbook over `items`, grouped into pages the way discovery
/// groups them.
fn discovered(items: Vec<DiscoveredItem>) -> DiscoveredScrapbook {
    let records = items.len();
    let mut by_page: BTreeMap<u32, Vec<DiscoveredItem>> = BTreeMap::new();
    for item in items {
        by_page.entry(item.page).or_default().push(item);
    }
    DiscoveredScrapbook {
        install_sha256: "install".to_owned(),
        content_sha256: "content".to_owned(),
        member_sha256: "member".to_owned(),
        member_span: SourceSpan::new(
            ContentHash::from_bytes([0x11; 32]),
            "GOSDATA/ASSETS/crimson.rof",
            Some("ASSETS/SCRAPBOOK.CSV"),
            0,
            records as u64,
            None,
        )
        .expect("a valid synthetic span"),
        records,
        pages: by_page
            .into_iter()
            .map(|(page, items)| DiscoveredPage { page, items })
            .collect(),
        gaps: BTreeMap::new(),
    }
}

/// A resolver with no resource header behind it: every name is unresolved.
fn no_names() -> impl Fn(&[u8]) -> Option<u32> {
    |_| None
}

/// A `has` that answers true to every fact: only a `Known` rule may unlock
/// against it.
fn all_facts(_: &UnlockFact) -> bool {
    true
}

// --------------------------------------------------------------- tests ---

/// Every discovered item becomes one declared entry, keyed so the audit and
/// F14-D.8's baseline rows name it identically, in the table's own order.
#[test]
fn accept_f47_e_every_discovered_item_becomes_a_declared_entry() {
    let table = discovered(vec![
        item("0_0_0", 0, "IDS_FIRST", None, 10),
        item("0_0_1", 0, "0", None, 11),
        item("1_0_0", 1, "IDS_SECOND", None, 20),
    ]);

    let declared = declare(&table, &no_names()).expect("the table declares");

    let ids: Vec<String> = declared
        .catalog
        .entries()
        .map(|entry| entry.id.as_str().to_owned())
        .collect();
    assert_eq!(
        ids,
        [
            "scrapbook_item/0_5f_0_5f_0",
            "scrapbook_item/0_5f_0_5f_1",
            "scrapbook_item/1_5f_0_5f_0",
        ],
        "one entry per item, keyed by install_file_key(entry_key), in table order"
    );
    for entry in declared.catalog.entries() {
        assert_eq!(entry.id.kind(), ContentKind::ScrapbookItem);
        assert_eq!(
            entry.kind,
            EntryKind::Page,
            "the unmeasured discriminator keeps every record a page"
        );
        assert_eq!(entry.visibility, EntryVisibility::Shown);
        assert_eq!(entry.title.kind(), ContentKind::StringResource);
        assert!(entry.replay.is_none(), "the table names no replay mission");
        assert!(!entry.unlock.is_known(), "no rule is invented");
    }
    assert!(
        declared
            .catalog
            .entries()
            .all(|entry| entry.kind != EntryKind::Memento),
        "no memento is declared without original evidence"
    );

    // The audit joins declaration to discovery on identity: nothing is
    // undeclared, nothing is fabricated.
    let report = audit(&table, &declared.catalog, &Catalog::new());
    assert_eq!(report.declared, 3);
    assert_eq!(report.declared_matched, 3);
    assert_eq!(report.undeclared_items, 0);
    assert!(report.declared_without_original.is_empty());
}

/// `TitleResID` is a resource **name**: defined names become
/// `string_resource/<n>` through the resolver; the table's `"0"` and empty
/// spellings become `string_resource/0`; a name the header does not answer
/// keeps its own escaped bytes in the key and is reported, never resolved to
/// a number it does not have.
#[test]
fn accept_f47_e_titles_resolve_through_the_resource_name_join() {
    let names: BTreeMap<Vec<u8>, u32> = [(b"IDS_KNOWN".to_vec(), 42_u32)].into_iter().collect();
    let resolve = |name: &[u8]| names.get(name).copied();
    let table = discovered(vec![
        item("0_0_0", 0, "IDS_KNOWN", None, 1),
        item("0_0_1", 0, "IDS_UNDEFINED", None, 2),
        item("0_0_2", 0, "0", None, 3),
        item("0_0_3", 0, "", None, 4),
    ]);

    let declared = declare(&table, &resolve).expect("the table declares");

    let titles: Vec<String> = declared
        .catalog
        .entries()
        .map(|entry| entry.title.as_str().to_owned())
        .collect();
    assert_eq!(
        titles,
        [
            "string_resource/42",
            "string_resource/ids_5f_undefined",
            "string_resource/0",
            "string_resource/0",
        ],
        "defined names resolve, null spellings are the null resource, an \
         undefined name keeps its own bytes"
    );
    assert_eq!(
        declared.unresolved_titles,
        [declared
            .catalog
            .entries()
            .nth(1)
            .expect("the item")
            .id
            .clone()],
        "exactly the undefined name is reported unresolved"
    );
}

/// An item whose picture the installation holds no decodable member for keeps
/// `image: None` — the state the archive carries — while a held member keys
/// its `image` id by the member spelling.
#[test]
fn accept_f47_e_a_missing_picture_is_declared_absent() {
    let held = "ASSETS/GRAPHICS/SCRAPBOOK/DZ_A.BMP";
    let table = discovered(vec![
        item("0_0_0", 0, "0", Some(held), 1),
        item("0_0_1", 0, "0", None, 2),
    ]);

    let declared = declare(&table, &no_names()).expect("the table declares");
    let mut entries = declared.catalog.entries();

    let with = entries.next().expect("the held item");
    assert_eq!(
        with.image.as_ref().map(|id| id.as_str()),
        Some(format!("image/{}", install_file_key(held)).as_str()),
        "the held picture's id is the member's install key"
    );
    assert_eq!(
        with.image.as_ref().map(|id| id.kind()),
        Some(ContentKind::Image)
    );

    let without = entries.next().expect("the missing-picture item");
    assert_eq!(
        without.image, None,
        "no placeholder stands in for a picture the archive does not hold"
    );
}

/// A record whose key never became an id key — the table can hold a non-UTF-8
/// key the schema cannot name — fails the whole build rather than being
/// silently dropped or misdeclared.
#[test]
fn accept_f47_e_a_record_the_schema_does_not_cover_fails_the_build() {
    let mut uncovered = item("1_0_0", 1, "0", None, 20);
    uncovered.id_key = None;
    let table = discovered(vec![item("0_0_0", 0, "0", None, 10), uncovered]);

    let error = declare(&table, &no_names()).unwrap_err();
    assert_eq!(
        error,
        DeclareError::UncoveredItem {
            key: "1_0_0".to_owned()
        },
        "the uncovered record is named, and no partial catalog is handed out"
    );
}

/// Two records spelling one id key is a declaration collision, not a merge:
/// the catalog's own validation refuses it.
#[test]
fn accept_f47_e_a_duplicate_item_key_fails_the_build() {
    let table = discovered(vec![
        item("0_0_0", 0, "0", None, 10),
        item("0_0_0", 0, "0", None, 11),
    ]);

    let error = declare(&table, &no_names()).unwrap_err();
    assert!(
        matches!(error, DeclareError::Catalog(_)),
        "the duplicate is the catalog's own refusal, not a skipped record: {error}"
    );
}

/// An unmeasured `Objective` declares no rule: the unknown stays unknown, and
/// unknown never unlocks — not even against a `has` that answers everything —
/// while `Shown` keeps every entry visible as locked.
#[test]
fn accept_f47_e_an_unknown_rule_never_unlocks_but_stays_visible() {
    let table = discovered(vec![
        item("0_0_0", 0, "0", None, 10),
        item("0_0_1", 0, "0", None, 11),
    ]);

    let declared = declare(&table, &no_names()).expect("the table declares");

    assert!(
        declared.catalog.unlocked(&all_facts).is_empty(),
        "an unknown rule never holds, even when every fact is present"
    );
    let visible: Vec<bool> = declared
        .catalog
        .visible(&all_facts)
        .map(|(_, unlocked)| unlocked)
        .collect();
    assert_eq!(
        visible,
        [false, false],
        "every entry shows locked: the book is declared, nothing is earned"
    );
}

/// The full production chain over the owner's installation: discover,
/// resolve names through the original's own `RESOURCE.H`, declare, audit.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f47_e_retail_installation_declares_the_whole_table() {
    let game_dir =
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must point at the owner's installation");
    let install_root = Path::new(&game_dir);

    let discovered = DiscoveredScrapbook::discover(install_root)
        .expect("the production discovery reads the retail table");
    let names = resource_names(install_root)
        .expect("the original's resource header reads through the same archive");
    let declared =
        declare(&discovered, &|name| names.get(name).copied()).expect("the retail table declares");

    assert_eq!(
        declared.catalog.entries().count(),
        461,
        "every discovered record becomes an entry"
    );
    assert_eq!(
        declared.catalog.entries().count(),
        discovered.paged_items(),
        "the declaration covers the discovery exactly"
    );
    // Twelve `TitleResID` names are dead references in the shipped table:
    // they appear in no `*.H` member of either installed container, so the
    // original's own headers could never have resolved them either. Their
    // entries keep the name in the title key and the build reports them.
    let unresolved: Vec<String> = declared
        .unresolved_titles
        .iter()
        .map(|id| {
            declared
                .catalog
                .entry(id)
                .expect("declared")
                .title
                .key()
                .to_owned()
        })
        .collect();
    let mut expected_unresolved: Vec<String> = [
        "IDS_SB_08_01_MANIFEST_T",
        "IDS_SB_09_01_TELEGRAM1_T",
        "IDS_SB_09_01_TELEGRAM2_T",
        "IDS_SB_12_01_FENCE_T",
        "IDS_SB_14_01_TELEGRAM3_T",
        "IDS_SB_16_01_SPARKSNOTE_T",
        "IDS_SB_21_01_BILLBILL_T",
        "IDS_SB_21_01_CONLETTER_T",
        "IDS_SB_21_01_CRAWNOTE_T",
        "IDS_SB_22_01_COMICLETTER_T",
        "IDS_SB_22_01_RICHLETTER_T",
        "IDS_SB_24_01_CRAWNOTE2_T",
    ]
    .iter()
    .map(|name| install_file_key(name))
    .collect();
    expected_unresolved.sort();
    let mut measured_unresolved = unresolved;
    measured_unresolved.sort();
    assert_eq!(
        measured_unresolved, expected_unresolved,
        "the 12 names no shipped header defines stay named in the catalog"
    );
    assert_eq!(
        declared
            .catalog
            .entries()
            .filter(
                |entry| entry.title.key().chars().all(|c| c.is_ascii_digit())
                    && entry.title.key() != "0"
            )
            .count(),
        66,
        "the names the headers do define resolve to string-table numbers"
    );
    assert_eq!(
        declared
            .catalog
            .entries()
            .filter(|entry| entry.image.is_some())
            .count(),
        294,
        "the items whose picture the installation holds"
    );
    assert_eq!(
        declared
            .catalog
            .entries()
            .filter(|entry| entry.image.is_none())
            .count(),
        167,
        "the `Snap_*` items declare no placeholder picture"
    );
    assert_eq!(
        declared
            .catalog
            .entries()
            .filter(|entry| entry.title.key() == "0")
            .count(),
        383,
        "the table's `0` and empty `TitleResID` spellings are the null resource"
    );
    for entry in declared.catalog.entries() {
        assert_eq!(entry.kind, EntryKind::Page);
        assert!(
            !entry.unlock.is_known(),
            "{}: no rule is invented",
            entry.id
        );
        assert!(
            entry.replay.is_none(),
            "{}: the table names no replay mission",
            entry.id
        );
    }
    assert!(
        declared.catalog.unlocked(&all_facts).is_empty(),
        "unknown rules never unlock"
    );
    assert_eq!(
        declared.catalog.visible(&all_facts).count(),
        461,
        "every entry stays visible as locked"
    );

    // The audit the task moves: declared and undeclared now name the real
    // catalog, and the report stays incomplete for the honest reason — the
    // unlock rules are unmeasured, not missing entries.
    let baseline = retail_baseline(install_root).expect("the production baseline reads");
    let report = audit(&discovered, &declared.catalog, &baseline.catalog);
    assert_eq!(report.declared, 461, "the runtime declares every item");
    assert_eq!(report.declared_matched, 461);
    assert_eq!(
        report.undeclared_items, 0,
        "no original item is undeclared any more"
    );
    assert!(
        report.declared_without_original.is_empty(),
        "nothing is declared the table does not hold"
    );
    assert_eq!(report.unlock_known, 0);
    assert_eq!(report.unlock_unknown, 461);
    assert_eq!(report.declared_mementos, 0, "no memento without evidence");
    assert!(
        !report.is_complete(),
        "the audit stays open: the unlock rules are unmeasured"
    );
}
