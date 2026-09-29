//! `accept_f12_i_*` tests for the documented record kinds and field lists
//! (task #371). The fixtures are authored from the *structure* the members
//! document; no original line is copied.

use super::records::{
    BOOL_MARKER_NOTE, BUTTON_COLOR_NOTE, RecordKind, documented_fields,
    documented_scrapbook_fields, optional_field_count,
};

/// The documented field lists are transcribed whole: one list per kind,
/// with the counts the members' comments imply, the button's four optional
/// colours, and the letter that selects each kind.
#[test]
fn accept_f12_i_documented_field_lists_are_transcribed() {
    let expected: [(RecordKind, usize); 11] = [
        (RecordKind::Button, 22),
        (RecordKind::Pane, 10),
        (RecordKind::Text, 9),
        (RecordKind::EditBox, 13),
        (RecordKind::Movie, 9),
        (RecordKind::TextList, 10),
        (RecordKind::ScrollingText, 14),
        (RecordKind::Dropdown, 14),
        (RecordKind::Listbox, 12),
        (RecordKind::Slider, 13),
        (RecordKind::SoundObject, 6),
    ];
    assert_eq!(RecordKind::ALL.len(), expected.len());
    let mut letters = Vec::new();
    for (kind, count) in expected {
        assert_eq!(
            documented_fields(kind).len(),
            count,
            "{} documented field count",
            kind.documented_name()
        );
        assert_eq!(RecordKind::from_letter(kind.letter()), Some(kind));
        assert!(!letters.contains(&kind.letter()), "letters are unique");
        letters.push(kind.letter());
        assert!(!kind.documented_name().is_empty());
    }
    // The one bracket the comments use marks the button's four colours.
    assert_eq!(optional_field_count(RecordKind::Button), 4);
    for kind in RecordKind::ALL {
        let optional = documented_fields(kind)
            .iter()
            .filter(|field| field.optional)
            .count();
        assert_eq!(optional, optional_field_count(kind));
        if kind != RecordKind::Button {
            assert_eq!(optional, 0);
        }
    }
    let colours: Vec<&str> = documented_fields(RecordKind::Button)
        .iter()
        .filter(|field| field.optional)
        .map(|field| field.name)
        .collect();
    assert_eq!(
        colours,
        vec![
            "ColorDisabled",
            "ColorActive",
            "ColorRollover",
            "ColorDepressed"
        ]
    );
    assert_eq!(
        documented_fields(RecordKind::Button)
            .iter()
            .find(|field| field.name == "ColorActive")
            .unwrap()
            .note,
        BUTTON_COLOR_NOTE
    );
    // Every `?`-suffixed field carries the member's own boolean note.
    let bool_named: Vec<&str> = documented_fields(RecordKind::Button)
        .iter()
        .filter(|field| field.name.ends_with('?'))
        .map(|field| field.name)
        .collect();
    assert_eq!(bool_named, vec!["EndScript?", "Checked?"]);
    assert_eq!(
        documented_fields(RecordKind::Button)
            .iter()
            .find(|field| field.name == "Checked?")
            .unwrap()
            .note,
        BOOL_MARKER_NOTE
    );
    for kind in RecordKind::ALL {
        for field in documented_fields(kind) {
            if field.name.ends_with('?') {
                assert_eq!(field.note, BOOL_MARKER_NOTE, "{}", field.name);
            }
        }
    }
    // The button-type field has no name of its own; the comment gives it a
    // value enum instead, kept verbatim so nothing is invented.
    assert!(
        documented_fields(RecordKind::Button)
            .iter()
            .any(|field| field.name == "(0=Normal,1=Check,2=Radio)")
    );
    // `from_letter` rejects a byte no documented kind uses.
    assert_eq!(RecordKind::from_letter(b'X'), None);
    assert_eq!(
        RecordKind::from_letter(b'b'),
        None,
        "record letters are upper case"
    );
}

/// `SCRAPBOOK.CSV` documents one `Mission_Spread_Item` list of **sixteen**
/// fields, the eleventh a quoted `"a,b,c,d"`. The "seventeen" the task
/// record uses counts that group as four names; the data is decisive.
#[test]
fn accept_f12_i_scrapbook_documents_sixteen_fields() {
    let fields = documented_scrapbook_fields();
    assert_eq!(fields.len(), 16);
    assert_eq!(fields[0].name, "Objective");
    assert_eq!(fields[10].name, "Left,Top,Right,Bottom");
    assert!(
        fields[10].note.contains("quoted"),
        "the group is one quoted field, not four"
    );
    assert_eq!(fields[15].name, "TextResID");
    assert!(fields.iter().all(|field| !field.optional));
}
