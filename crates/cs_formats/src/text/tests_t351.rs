//! `accept_t351_*`: the established reading rules of the keyed field list
//! dialect, and the retail correspondence they rest on (task #351).
//!
//! Four rules are established by the retail data alone and are exercised
//! here: a name is compared without regard to ASCII case (**R1**), the blank
//! bytes before a marker are not part of the name (**R2**), the blank bytes
//! around a key are not part of it (**R3**), and the blank bytes around a
//! field are dropped before the field is used (**R4**). The findings, the
//! measurements and the rules that stayed unknown are in
//! `docs/findings/2026-09-29-t351-keyed-list-reading-rules.md`.
//!
//! The two retail tests are `#[ignore = "requires CS_GAME_DIR"]` and fail
//! loudly without it. The scripted fixtures are authored: they follow the
//! observed shapes and copy no original line.

use crate::ParseContext;

use super::LineKind;
use super::dialect::{CRIMSON_ROF, MemberRule, TextDialect, dialect_for_member};
use super::keyed_list::{Entry, Fields, KeyedList, read_keyed_list};
use super::resource_header::{HeaderLookup, ResourceHeader, read_resource_header};
use super::tests::{retail_member, retail_rof, rof_entries};

/// Authored for **R4**: an unquoted field padded at both ends, one padded
/// only at the end, a field of blanks only, and a quoted field whose blanks
/// are *inside* its quotes. (A quoted field can never carry blanks *outside*
/// its quotes: the reader refuses a value whose closing quote is not
/// immediately followed by a `,` or the end, keeping it as
/// [`super::QuoteIssue::TextAfterClosingQuote`].)
const PADDED: &[u8] = b"[BOOK]\r\n\
ITEM=  0  ,  IDS_TITLE  ,\"0, 0,0\",  ,  \r\n\
EMPTY=   \r\n";

fn read(bytes: &[u8]) -> KeyedList<'_> {
    let mut context = ParseContext::with_defaults("fixture.csv");
    read_keyed_list(&mut context, bytes).expect("an authored list reads")
}

fn entry<'a>(list: &'a KeyedList<'a>, key: &[u8]) -> &'a Entry<'a> {
    list.entries()
        .map(|(_, _, entry)| entry)
        .find(|entry| entry.key == key)
        .expect("the fixture has this key")
}

fn fields<'a>(entry: &'a Entry<'a>) -> &'a [super::Field<'a>] {
    match &entry.fields {
        Fields::Split(fields) => fields,
        Fields::Unsplit { issue, .. } => panic!("unexpected {}", issue.code()),
    }
}

fn values<'a>(entry: &'a Entry<'a>) -> Vec<&'a [u8]> {
    fields(entry).iter().map(|field| field.value()).collect()
}

fn texts<'a>(entry: &'a Entry<'a>) -> Vec<&'a [u8]> {
    fields(entry).iter().map(|field| field.text).collect()
}

/// **R4**: a consumer compares the field without the blank bytes around it,
/// while every byte stays in `raw` and in the line, so the member still
/// reassembles exactly.
#[test]
fn accept_t351_field_value_drops_surrounding_blanks() {
    let list = read(PADDED);
    let item = entry(&list, b"ITEM");
    assert_eq!(
        item.value, b"  0  ,  IDS_TITLE  ,\"0, 0,0\",  ,  ",
        "the value keeps the bytes as written"
    );
    assert_eq!(
        values(item),
        vec![&b"0"[..], b"IDS_TITLE", b"0, 0,0", b"", b""],
        "**R4**: the blanks around each field are dropped"
    );
    assert_eq!(
        texts(item),
        vec![&b"  0  "[..], b"  IDS_TITLE  ", b"0, 0,0", b"  ", b"  "],
        "`text` removes the enclosing quotes only, so it still holds the \
         padding and is not the form a consumer compares"
    );
    let item_fields = fields(item);
    assert_eq!(item_fields[0].raw, b"  0  ");
    assert!(!item_fields[0].quoted);
    assert!(!item_fields[1].quoted);
    assert!(item_fields[2].quoted);
    assert_eq!(
        item_fields[2].value(),
        b"0, 0,0",
        "a blank inside the quotes stays"
    );
    assert!(!item_fields[3].quoted);
    assert_eq!(
        item_fields[3].value(),
        b"",
        "a blank field is an empty value"
    );

    // A field of blanks only, and an empty value, are one empty field.
    let empty = entry(&list, b"EMPTY");
    assert_eq!(empty.value, b"   ");
    assert_eq!(values(empty), vec![&b""[..]]);

    // The two shapes a quoted field can never have: blanks between the
    // closing quote and the `,`, and blanks before the opening quote. Both
    // are quoting the survey never observed, so the value stays whole
    // instead of being split and trimmed.
    let after = read(b"BOOK=X,\"a\"  ,c\r\n");
    assert_eq!(
        entry(&after, b"BOOK").fields,
        Fields::Unsplit {
            issue: super::QuoteIssue::TextAfterClosingQuote,
            at: 4
        }
    );
    assert_eq!(after.reassemble(), b"BOOK=X,\"a\"  ,c\r\n");
    let before = read(b"BOOK=X,\"a\",  \"b\"  ,c\r\n");
    assert_eq!(
        entry(&before, b"BOOK").fields,
        Fields::Unsplit {
            issue: super::QuoteIssue::QuoteInsideField,
            at: 8
        }
    );
    assert_eq!(before.reassemble(), b"BOOK=X,\"a\",  \"b\"  ,c\r\n");

    assert_eq!(list.reassemble(), PADDED, "every byte survives");
}

/// The retail correspondence the three name rules rest on, read through the
/// production reader and the production member reader: every one of the 34
/// `[@…@]` sections is named by a `.SCRIPT` member in a different case and
/// none exactly, and none of the 402 object names the 34 UI scripts write by
/// hand matches an entry key exactly — every one of them matches only when
/// ASCII case is folded, 271 of them sit on a key padded before its `=` and
/// 145 on an indented line.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t351_retail_object_names_resolve_only_case_insensitively() {
    let rof = retail_rof();
    let bytes = retail_member(&rof, "ASSETS/LAYOUT.CSV");
    let mut context = ParseContext::with_defaults("ASSETS/LAYOUT.CSV");
    let layout = read_keyed_list(&mut context, &bytes).expect("the layout reads");

    // One row per section: its name as written, and one row per entry with
    // the key, whether the key was padded before its `=` and whether the
    // line was indented — the two things **R2** and **R3** are about.
    let mut sections: Vec<(Vec<u8>, Vec<Row>)> = Vec::new();
    for line in layout.lines() {
        match &line.kind {
            LineKind::Section { name } => sections.push((name.to_vec(), Vec::new())),
            LineKind::Entry(entry) => {
                let last = sections
                    .last_mut()
                    .expect("the surveyed members open with a header");
                last.1.push((
                    entry.key.to_vec(),
                    entry.separator > entry.key_range.end,
                    !line.indent.is_empty(),
                ));
            }
            _ => {}
        }
    }
    let wrapped = sections
        .iter()
        .filter(|(name, _)| name.starts_with(b"@") && name.ends_with(b"@"))
        .count();
    assert_eq!(wrapped, 34, "34 `[@…@]` sections and one GLOBALVARS");

    let MemberRule::Directory {
        directory, count, ..
    } = TextDialect::UiScript.record().members[0]
    else {
        panic!("the script row is a directory rule")
    };
    // The directory also holds the two `.H` headers and `DEBUGINFO.TXT`, so
    // the members are the ones the inventory routes to the script dialect.
    let scripts: Vec<String> = rof_dir_entries(&rof, directory.trim_end_matches('/'))
        .into_iter()
        .filter(|name| {
            dialect_for_member(CRIMSON_ROF, Some(&format!("{directory}{name}")))
                == Some(TextDialect::UiScript)
        })
        .collect();
    assert_eq!(
        scripts.len(),
        usize::try_from(count).expect("the surveyed count fits a usize"),
        "the surveyed script count"
    );

    let mut sections_named_by_a_script = 0usize;
    let mut exact_section_names = 0usize;
    let mut bound = 0usize;
    let mut bound_differing_in_case = 0usize;
    let mut bound_on_a_padded_key = 0usize;
    let mut bound_on_an_indented_line = 0usize;
    let mut dynamic = 0usize;
    let mut joined = 0usize;
    let mut joined_matching_exactly = 0usize;
    let mut dangling: Vec<(String, String)> = Vec::new();

    for script in &scripts {
        let base = script.trim_end_matches(".SCRIPT");
        let member = format!("{directory}{script}");
        let bytes = retail_member(&rof, &member);
        let mut section: Option<&Vec<Row>> = None;
        for (name, entries) in &sections {
            let inner = name
                .strip_prefix(b"@")
                .and_then(|rest| rest.strip_suffix(b"@"))
                .unwrap_or(name);
            if inner.eq_ignore_ascii_case(base.as_bytes()) {
                section = Some(entries);
            }
        }
        let references = object_names(&bytes);
        let Some(entries) = section else {
            assert!(
                references.is_empty(),
                "{base}: a script with no section binds no object"
            );
            continue;
        };
        sections_named_by_a_script += 1;
        if sections
            .iter()
            .any(|(name, _)| name == format!("[@{base}@]").as_bytes())
        {
            exact_section_names += 1;
        }
        for reference in references {
            match reference {
                Reference::Joined { variable, suffix } => {
                    // The name is built at run time out of a variable and a
                    // literal suffix, so only the suffix is spelled here.
                    let folded = suffix.to_ascii_lowercase().into_bytes();
                    let matching = entries
                        .iter()
                        .filter(|(key, _, _)| key.to_ascii_lowercase().ends_with(&folded))
                        .count();
                    joined += 1;
                    assert!(matching > 0, "{base}: no key ends with {suffix:?}");
                    joined_matching_exactly += entries
                        .iter()
                        .filter(|(key, _, _)| key.ends_with(suffix.as_bytes()))
                        .count();
                    let _ = variable;
                }
                Reference::Literal(name) => {
                    let wanted = name.to_ascii_lowercase().into_bytes();
                    let found = entries
                        .iter()
                        .find(|(key, _, _)| key.to_ascii_lowercase() == wanted);
                    let Some((key, padded, indented)) = found else {
                        // A name the script builds at run time is a prefix of
                        // one or more keys; anything else is a name no entry
                        // carries.
                        if entries
                            .iter()
                            .any(|(key, _, _)| key.to_ascii_lowercase().starts_with(&wanted))
                        {
                            dynamic += 1;
                        } else {
                            dangling.push((base.to_owned(), name));
                        }
                        continue;
                    };
                    bound += 1;
                    if key.as_slice() != name.as_bytes() {
                        bound_differing_in_case += 1;
                    }
                    bound_on_a_padded_key += usize::from(*padded);
                    bound_on_an_indented_line += usize::from(*indented);
                }
            }
        }
    }

    assert_eq!(sections_named_by_a_script, 34, "each has a script");
    assert_eq!(exact_section_names, 0, "no section name matches exactly");
    assert_eq!(bound, 402, "object names written by hand");
    assert_eq!(bound_differing_in_case, bound, "none matches exactly");
    assert_eq!(bound_on_a_padded_key, 271, "keys padded before their `=`");
    assert_eq!(bound_on_an_indented_line, 145, "indented entry lines");
    assert_eq!(dynamic, 35, "names the scripts build at run time");
    assert_eq!(joined, 5, "names joined with a variable");
    assert_eq!(joined_matching_exactly, 0, "no suffix matches exactly");
    assert_eq!(
        dangling,
        vec![("MAINMENU".to_owned(), "mm_t_title".to_owned())],
        "the one bound name no entry carries"
    );
}

/// **R4** on the retail member that has the padded fields: exactly seven
/// fields of `SCRAPBOOK.CSV` carry blank bytes around them, and each of the
/// seven is a resource-id name that one of the two `.H` members defines byte
/// for byte without the padding — which is why the blanks have to go before
/// a consumer compares a field against a header.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t351_retail_padded_fields_name_defined_resource_ids() {
    let rof = retail_rof();
    let members = ["ASSETS/SCRIPTS/RESOURCE.H", "ASSETS/SCRIPTS/RESRC1.H"];
    let members_bytes: Vec<Vec<u8>> = members
        .iter()
        .map(|member| retail_member(&rof, member))
        .collect();
    let mut headers: Vec<ResourceHeader<'_>> = Vec::new();
    for (member, bytes) in members.iter().zip(&members_bytes) {
        let mut context = ParseContext::with_defaults(*member);
        headers.push(read_resource_header(&mut context, bytes).expect("a header reads"));
    }
    assert!(
        headers
            .iter()
            .all(|header| header.unclassified().count() == 0),
        "the surveyed headers have no line the reader does not explain"
    );

    let bytes = retail_member(&rof, "ASSETS/SCRAPBOOK.CSV");
    let mut context = ParseContext::with_defaults("ASSETS/SCRAPBOOK.CSV");
    let book = read_keyed_list(&mut context, &bytes).expect("the scrapbook reads");

    let mut quoted_fields = 0usize;
    let mut padded: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    for (_, _, entry) in book.entries() {
        for field in fields(entry) {
            if field.quoted {
                quoted_fields += 1;
            } else if field.raw != field.value() {
                padded.push((entry.key.to_vec(), field.value().to_vec()));
            }
        }
    }
    assert_eq!(
        quoted_fields, 461,
        "the quoted fields of the member's entries"
    );
    assert_eq!(padded.len(), 7, "the padded fields of the member");
    for (key, name) in &padded {
        assert!(
            name.starts_with(b"IDS_"),
            "{key:?}: {} is a resource-id name",
            String::from_utf8_lossy(name)
        );
        assert!(
            headers
                .iter()
                .any(|header| matches!(header.lookup(name), HeaderLookup::Found(_))),
            "{key:?}: a `.H` member defines {} byte for byte",
            String::from_utf8_lossy(name)
        );
    }
    assert_eq!(book.reassemble(), bytes, "every byte survives");
}

/// One entry of a section, reduced to what the three name rules are about:
/// the key as written, whether the key was padded with blank bytes before its
/// `=` (**R3**) and whether the line was indented (**R2**).
type Row = (Vec<u8>, bool, bool);

/// One object name a script binds, in the two spellings the surveyed scripts
/// use: a whole literal, or a variable followed by a literal suffix that the
/// interpreter joins at run time.
enum Reference {
    /// `object.YC = "name"`.
    Literal(String),
    /// `object.YC = VARIABLE "suffix"`.
    Joined {
        /// The variable the name starts with; its value is not read here.
        variable: String,
        /// The literal part of the name.
        suffix: String,
    },
}

/// The names the surveyed scripts bind an object by: the string of every
/// `.YC = …` assignment. The scripts are a different dialect (F13 owns them)
/// and are read here only as bytes.
fn object_names(bytes: &[u8]) -> Vec<Reference> {
    let mut names = Vec::new();
    let mut at = 0usize;
    while let Some(found) = bytes[at..].windows(3).position(|window| window == b".YC") {
        // The assignment that follows: `=` and then either a quote or an
        // identifier and a quote.
        let mut cursor = at + found + 3;
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if bytes.get(cursor) != Some(&b'=') {
            at += found + 3;
            continue;
        }
        cursor += 1;
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        let variable_start = cursor;
        while bytes
            .get(cursor)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        {
            cursor += 1;
        }
        let variable = String::from_utf8_lossy(&bytes[variable_start..cursor]).into_owned();
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        let Some(length) = quoted(bytes, cursor) else {
            at += found + 3;
            continue;
        };
        let name = String::from_utf8_lossy(&bytes[cursor + 1..cursor + length]).into_owned();
        names.push(if variable.is_empty() {
            Reference::Literal(name)
        } else {
            Reference::Joined {
                variable,
                suffix: name,
            }
        });
        at = cursor + length;
    }
    names
}

/// The bytes between the quote at `at` and the quote that closes it, or
/// `None` when there is none.
fn quoted(bytes: &[u8], at: usize) -> Option<usize> {
    if bytes.get(at) != Some(&b'"') {
        return None;
    }
    let end = bytes[at + 1..].iter().position(|byte| *byte == b'"')?;
    Some(end + 1)
}

/// The member names of one directory of `crimson.rof`, read with the
/// production directory reader one block at a time (the same walk
/// [`retail_member`] makes, stopping at the directory).
fn rof_dir_entries(rof: &[u8], path: &str) -> Vec<String> {
    let segments: Vec<&str> = path.split('/').collect();
    let mut offset = 0u32;
    for (depth, segment) in segments.iter().enumerate() {
        let (name, record) = rof_entries(rof, offset)
            .into_iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(segment))
            .unwrap_or_else(|| panic!("{path}: no `{segment}` in the retail archive"));
        if depth + 1 == segments.len() {
            return rof_entries(rof, record.start)
                .into_iter()
                .map(|(name, _)| name)
                .collect();
        }
        assert!(record.flags.is_directory(), "{path}: {name} is a directory");
        offset = record.start;
    }
    unreachable!("paths have at least one segment")
}
