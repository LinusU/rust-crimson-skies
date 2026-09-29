//! `accept_f12_e_*`: the `<NAME>` placeholder pass of the keyed field list
//! dialect (task #370, keyed `F12-E`).
//!
//! The rule and every number the retail test pins were measured by task #351
//! (`docs/findings/2026-09-29-t351-keyed-list-reading-rules.md`); this stage
//! implements the pass and its findings are recorded in
//! `docs/findings/2026-09-29-f12-e-name-placeholders.md`. The scripted
//! fixture is authored: it follows the observed shapes and copies no original
//! line. The retail test is `#[ignore = "requires CS_GAME_DIR"]` and fails
//! loudly without it.

use std::collections::BTreeSet;

use crate::{ParseContext, ParseErrorKind};

use super::keyed_list::read_keyed_list;
use super::placeholder::{PlaceholderScope, definition_scope, placeholder_name, read_placeholders};
use super::tests::{retail_member, retail_rof};

/// Authored for the scope rules: two global definitions, a local definition
/// of a name a global also spells (so the local table answering first is
/// visible), a reference no table can answer, a field that mixes a
/// placeholder with other text and a quoted placeholder — the last two the
/// whole-field rule must leave alone.
const MEMBER: &[u8] = b";placeholders, authored\r\n\
[GLOBALVARS]\r\n\
G1=WIDTH,640\r\n\
G2=HEIGHT,480\r\n\
G3=SHARED,global\r\n\
[@Panel@]\r\n\
V1=LEFT,10\r\n\
V2=SHARED,local\r\n\
PANEL_W=<WIDTH>\r\n\
PANEL_H=<HEIGHT>\r\n\
PANEL_L=<LEFT>\r\n\
PANEL_S=<SHARED>\r\n\
MISSING=<NOPE>\r\n\
EMBEDDED=<LEFT>-<WIDTH>\r\n\
PLAIN=hello\r\n\
QUOTED_FIELD=\"<LEFT>\"\r\n\
[@Other@]\r\n\
V3=ONLY,7\r\n\
OTHER_L=<LEFT>\r\n\
OTHER_S=<SHARED>\r\n\
NOTDEF=one\r\n\
V4=single\r\n";

fn table(bytes: &[u8]) -> super::placeholder::PlaceholderTable {
    let mut context = ParseContext::with_defaults("fixture.csv");
    let list = read_keyed_list(&mut context, bytes).expect("an authored list reads");
    read_placeholders(&mut context, &list).expect("the placeholder pass reads")
}

/// A reference resolves against its own section's `V` definitions first and
/// the `G` ones otherwise; the same name resolves locally in one section and
/// globally in another, and a local name never leaks into the next section.
#[test]
fn accept_f12_e_references_resolve_local_first_then_global() {
    let table = table(MEMBER);
    assert_eq!(
        table.accounting(),
        super::placeholder::PlaceholderAccounting {
            definitions: 6,
            local_definitions: 3,
            global_definitions: 3,
            references: 7,
            resolved_local: 2,
            resolved_global: 3,
            unresolved: 2,
        },
        "G1..G3, V1..V3; PANEL_W/H/S, PANEL_L, PANEL_S, MISSING, OTHER_L, OTHER_S"
    );

    let reference = |key: &[u8], field: usize| {
        table
            .references()
            .iter()
            .find(|reference| reference.key == key && reference.field == field)
            .expect("the fixture has this reference")
    };
    let resolved = |key: &[u8]| {
        table
            .resolved(reference(key, 0))
            .unwrap_or_else(|| panic!("{key:?} resolves"))
    };

    // A global definition from the `[GLOBALVARS]` section.
    assert_eq!(
        resolved(b"PANEL_W"),
        super::placeholder::ResolvedPlaceholder {
            scope: PlaceholderScope::Global,
            value: b"640",
        }
    );
    // A local definition of the reference's own section.
    assert_eq!(resolved(b"PANEL_L").value, b"10");
    assert_eq!(resolved(b"PANEL_L").scope, PlaceholderScope::Local);
    // The local table answers before the global one.
    assert_eq!(resolved(b"PANEL_S").scope, PlaceholderScope::Local);
    assert_eq!(resolved(b"PANEL_S").value, b"local");
    // The same name, in a section that does not spell it locally, takes the
    // global definition.
    assert_eq!(resolved(b"OTHER_S").scope, PlaceholderScope::Global);
    assert_eq!(resolved(b"OTHER_S").value, b"global");
    // A local definition does not leak into another section: `LEFT` is only
    // defined in `[@Panel@]`, so `[@Other@]`'s reference is unresolved.
    assert!(reference(b"OTHER_L", 0).definition.is_none());

    // The definitions carry the scope the key shape gave them, and a `V`
    // key with one field is not a definition.
    let definitions: Vec<_> = table.definitions().iter().map(|d| d.key.clone()).collect();
    assert_eq!(
        definitions,
        vec![
            b"G1".to_vec(),
            b"G2".to_vec(),
            b"G3".to_vec(),
            b"V1".to_vec(),
            b"V2".to_vec(),
            b"V3".to_vec(),
        ]
    );
    assert!(
        !table.definitions().iter().any(|d| d.key == b"V4"),
        "a one-field value is not a definition"
    );

    // The raw list is unchanged: the pass is a second pass.
    let mut context = ParseContext::with_defaults("fixture.csv");
    let list = read_keyed_list(&mut context, MEMBER).expect("reads");
    assert_eq!(list.reassemble(), MEMBER);
}

/// An unresolved name is reported, never guessed or dropped, and a field that
/// is not exactly one `<NAME>` is not a reference at all.
#[test]
fn accept_f12_e_unresolved_names_are_reported_not_guessed() {
    // The pass returns an owned table, so it outlives the borrowed list.
    let mut context = ParseContext::with_defaults("fixture.csv");
    let list = read_keyed_list(&mut context, MEMBER).expect("reads");
    let table = read_placeholders(&mut context, &list).expect("pass reads");

    let unresolved: Vec<(&[u8], usize)> = table
        .unresolved()
        .map(|reference| (reference.key.as_slice(), reference.field))
        .collect();
    assert_eq!(
        unresolved,
        vec![(&b"MISSING"[..], 0), (&b"OTHER_L"[..], 0)],
        "the names no definition carries"
    );
    let mut names: Vec<&[u8]> = table
        .unresolved()
        .map(|reference| reference.name.as_slice())
        .collect();
    names.sort_unstable();
    assert_eq!(names, vec![&b"LEFT"[..], b"NOPE"]);

    // A field that embeds a placeholder in other text, and a quoted
    // placeholder, are not references: the whole-field rule is the observed
    // one and nothing is substituted into a larger field.
    assert!(
        !table
            .references()
            .iter()
            .any(|reference| reference.key == b"EMBEDDED")
    );
    assert!(
        !table
            .references()
            .iter()
            .any(|reference| reference.key == b"QUOTED_FIELD")
    );

    // The lexical predicates.
    assert_eq!(definition_scope(b"V1"), Some(PlaceholderScope::Local));
    assert_eq!(definition_scope(b"G29"), Some(PlaceholderScope::Global));
    assert_eq!(definition_scope(b"V"), None);
    assert_eq!(definition_scope(b"V1x"), None);
    assert_eq!(definition_scope(b"Q1"), None);
    assert_eq!(placeholder_name(b"<A>"), Some(&b"A"[..]));
    assert_eq!(placeholder_name(b"<A_B2>"), Some(&b"A_B2"[..]));
    assert_eq!(placeholder_name(b"<>"), None);
    assert_eq!(placeholder_name(b"<A"), None);
    assert_eq!(placeholder_name(b"A>"), None);
    assert_eq!(placeholder_name(b"<A>B"), None);
    assert_eq!(placeholder_name(b"<A><B>"), None);
    assert_eq!(placeholder_name(b"<<A>>"), None);
    assert_eq!(placeholder_name(b"plain"), None);
}

/// Every table and every byte it owns is booked against the parse's
/// allocation budget: the exact charge reads and one byte less is refused,
/// booking nothing.
#[test]
fn accept_f12_e_the_pass_is_bounded_by_the_allocation_budget() {
    let mut list_context = ParseContext::with_defaults("fixture.csv");
    let _list = read_keyed_list(&mut list_context, MEMBER).expect("reads");
    let list_charge = list_context.allocation().used();

    let mut context = ParseContext::with_defaults("fixture.csv");
    let list = read_keyed_list(&mut context, MEMBER).expect("reads");
    read_placeholders(&mut context, &list).expect("pass reads");
    let exact = context.allocation().used();
    assert!(
        exact > list_charge,
        "the pass charges for its tables and the bytes they own"
    );

    let mut exact_context = ParseContext::new("exact", exact, 8);
    let list = read_keyed_list(&mut exact_context, MEMBER).expect("the list fits");
    read_placeholders(&mut exact_context, &list).expect("exactly the budget it books is enough");
    assert_eq!(exact_context.allocation().used(), exact);

    let mut short = ParseContext::new("short", exact - 1, 8);
    let list = read_keyed_list(&mut short, MEMBER).expect("the list itself fits");
    let error = read_placeholders(&mut short, &list).expect_err("one byte short is refused");
    assert_eq!(error.kind, ParseErrorKind::AllocationBudgetExceeded);
    assert_eq!(
        short.allocation().used(),
        list_charge,
        "the refused pass rolled its charges back; the list parse keeps its own"
    );
}

/// The retail correspondence: 1313 references, 186 definitions and every
/// reference resolved — the numbers task #351 measured, now read through the
/// pass. `SCRAPBOOK.CSV` has none.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f12_e_retail_layout_references_all_resolve() {
    let rof = retail_rof();
    let bytes = retail_member(&rof, "ASSETS/LAYOUT.CSV");
    let mut context = ParseContext::with_defaults("ASSETS/LAYOUT.CSV");
    let list = read_keyed_list(&mut context, &bytes).expect("the layout reads");
    let table = read_placeholders(&mut context, &list).expect("the pass reads");

    assert_eq!(
        table.accounting(),
        super::placeholder::PlaceholderAccounting {
            definitions: 186,
            local_definitions: 157,
            global_definitions: 29,
            references: 1313,
            resolved_local: 793,
            resolved_global: 520,
            unresolved: 0,
        }
    );

    // Every name is uppercase in the surveyed member, so the case fold this
    // pass does not apply is unobservable; and no local definition shadows a
    // global one, so the local-first order it does apply is unobservable too.
    let globals: BTreeSet<&[u8]> = table
        .definitions()
        .iter()
        .filter(|definition| definition.scope == PlaceholderScope::Global)
        .map(|definition| definition.name.as_slice())
        .collect();
    for definition in table.definitions() {
        assert!(
            definition
                .name
                .iter()
                .all(|byte| !byte.is_ascii_lowercase()),
            "{:?} is spelled in lower case",
            String::from_utf8_lossy(&definition.name)
        );
        if definition.scope == PlaceholderScope::Local {
            assert!(
                !globals.contains(definition.name.as_slice()),
                "{:?} shadows a global",
                String::from_utf8_lossy(&definition.name)
            );
        }
        assert!(
            placeholder_name(&definition.value).is_none(),
            "{:?} has a placeholder value: chained definitions were never observed",
            String::from_utf8_lossy(&definition.key)
        );
    }
    let distinct: BTreeSet<&[u8]> = table
        .references()
        .iter()
        .map(|reference| reference.name.as_slice())
        .collect();
    assert_eq!(distinct.len(), 113, "distinct reference names");

    // Every resolution names the definition it points at, and reading a
    // reference through the table returns that definition's value.
    for reference in table.references() {
        let definition = table
            .definition(reference)
            .expect("every retail reference resolves");
        assert_eq!(definition.name, reference.name);
        assert_eq!(
            table.resolved(reference).expect("resolves").value,
            definition.value
        );
    }

    assert_eq!(list.reassemble(), bytes, "every byte survives");

    // The other keyed-list member has no placeholder at all.
    let book = retail_member(&rof, "ASSETS/SCRAPBOOK.CSV");
    let mut context = ParseContext::with_defaults("ASSETS/SCRAPBOOK.CSV");
    let list = read_keyed_list(&mut context, &book).expect("the scrapbook reads");
    let table = read_placeholders(&mut context, &list).expect("the pass reads");
    assert_eq!(table.accounting(), Default::default());
    assert!(table.definitions().is_empty());
    assert!(table.references().is_empty());
}
