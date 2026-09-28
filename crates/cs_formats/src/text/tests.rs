//! `accept_f12_a_*`: lossless text lines, keyed field list nodes and the
//! dialect inventory, on newly authored fixtures.

use cs_types::evidence::ClaimStatus;

use super::dialect::{CRIMSON_ROF, DialectReader, MemberRule, TEXT_DIALECT_INVENTORY};
use super::*;
use crate::{ParseContext, ParseErrorKind};

/// An authored keyed field list exercising every observed lexical feature
/// plus the ones the reader must not guess about: a comment with quotes
/// and commas, a blank line, an indented comment, a section with a
/// trailing tab, a quoted field holding commas, empty fields, padding
/// between key and `=`, a Windows-1252 and a UTF-8 non-ASCII name, a quoted
/// `;`, a stray `:` line, bare `\n` and a last line without terminator.
const FIXTURE: &[u8] = b";; authored: \"quoted\", commas, ok\r\n\
\r\n\
[FIRST]\r\n\
\t;indented comment\r\n\
ALPHA=P,art.png,10,20,\"1,2,3,4\",,0x10\r\n\
\x20   BETA\t=B,,<SLOT>,0\r\n\
\t[SECOND]\t\r\n\
Caf\xe9=\xe9t\xe9,\"a;b\"\r\n\
Z\xc3\xa9ro=1\n\
:not an entry\r\n\
LAST=";

fn read(bytes: &[u8]) -> KeyedList<'_> {
    let mut context = ParseContext::with_defaults("fixture.list");
    read_keyed_list(&mut context, bytes).expect("an authored list reads")
}

fn entry<'a>(list: &'a KeyedList<'_>, key: &[u8]) -> &'a Entry<'a> {
    list.entries()
        .map(|(_, _, entry)| entry)
        .find(|entry| entry.key == key)
        .expect("the fixture has this key")
}

fn split<'a>(entry: &'a Entry<'_>) -> Vec<&'a [u8]> {
    match &entry.fields {
        Fields::Split(fields) => fields.iter().map(|field| field.text).collect(),
        Fields::Unsplit { issue, .. } => panic!("unexpected {}", issue.code()),
    }
}

/// AC01: a comma inside quotes does not split the field, the quotes are
/// kept in the raw field, and empty fields are fields.
#[test]
fn accept_f12_a_quoted_separators_stay_inside_one_field() {
    let list = read(FIXTURE);
    let alpha = entry(&list, b"ALPHA");
    assert_eq!(
        split(alpha),
        vec![
            &b"P"[..],
            b"art.png",
            b"10",
            b"20",
            b"1,2,3,4",
            b"",
            b"0x10"
        ]
    );
    let Fields::Split(fields) = &alpha.fields else {
        unreachable!()
    };
    assert!(fields[4].quoted);
    assert_eq!(fields[4].raw, b"\"1,2,3,4\"");
    assert_eq!(&alpha.value[fields[4].range.clone()], fields[4].raw);
    assert!(!fields[5].quoted);

    // A quoted `;` is a field, not a comment.
    let cafe = entry(&list, b"Caf\xe9");
    assert_eq!(split(cafe), vec![&b"\xe9t\xe9"[..], b"a;b"]);

    // An empty value is one empty field.
    assert_eq!(split(entry(&list, b"LAST")), vec![&b""[..]]);
}

/// AC01: whole-line comments, blank lines and indentation are nodes, with
/// their bytes.
#[test]
fn accept_f12_a_comments_blank_lines_and_sections_are_kept() {
    let list = read(FIXTURE);
    let kinds: Vec<_> = list.lines().iter().map(|line| &line.kind).collect();
    assert_eq!(
        kinds[0],
        &LineKind::Comment {
            text: b"; authored: \"quoted\", commas, ok"
        }
    );
    assert_eq!(kinds[1], &LineKind::Blank);
    assert_eq!(kinds[2], &LineKind::Section { name: b"FIRST" });
    assert_eq!(
        kinds[3],
        &LineKind::Comment {
            text: b"indented comment"
        }
    );
    assert_eq!(list.lines()[3].indent, b"\t");
    assert_eq!(kinds[6], &LineKind::Section { name: b"SECOND" });
    assert_eq!(list.lines()[6].content, b"\t[SECOND]\t");

    // Padding around the key is not part of the key, but stays in the line.
    let beta_line = &list.lines()[5];
    let LineKind::Entry(beta) = &beta_line.kind else {
        panic!("BETA is an entry")
    };
    assert_eq!(beta.key, b"BETA");
    assert_eq!(beta_line.indent, b"    ");
    assert_eq!(&beta_line.content[beta.key_range.clone()], b"BETA");
    assert_eq!(beta_line.content[beta.separator], b'=');
    assert_eq!(split(beta), vec![&b"B"[..], b"", b"<SLOT>", b"0"]);

    // Entries know the section they follow.
    let sections: Vec<_> = list
        .entries()
        .map(|(section, _, entry)| (section, entry.key))
        .collect();
    assert_eq!(
        sections,
        vec![
            (Some(2), &b"ALPHA"[..]),
            (Some(2), b"BETA"),
            (Some(6), b"Caf\xe9"),
            (Some(6), b"Z\xc3\xa9ro"),
            (Some(6), b"LAST"),
        ]
    );
}

/// AC01: CRLF, LF and a missing final terminator survive, with offsets,
/// and the nodes reassemble the input byte for byte.
#[test]
fn accept_f12_a_crlf_terminators_survive_and_reassemble() {
    let mut context = ParseContext::with_defaults("fixture.list");
    let lines = scan_lines(&mut context, FIXTURE).expect("scan");
    assert_eq!(
        lines.terminator_counts(),
        TerminatorCounts {
            crlf: 9,
            lf: 1,
            none: 1
        }
    );
    let terminators: Vec<_> = lines.lines().iter().map(|line| line.terminator).collect();
    assert_eq!(terminators[0], LineTerminator::CrLf);
    assert_eq!(terminators[8], LineTerminator::Lf);
    assert_eq!(terminators[10], LineTerminator::None);
    assert_eq!(lines.content(&lines.lines()[1]), b"");
    assert_eq!(lines.lines()[1].offset, 35);
    assert_eq!(lines.lines()[1].number, 2);
    // A CR is never part of the content it terminates.
    assert!(
        lines
            .lines()
            .iter()
            .all(|line| !lines.content(line).ends_with(b"\r"))
    );
    assert_eq!(lines.reassemble(), FIXTURE);
    assert_eq!(read(FIXTURE).reassemble(), FIXTURE);

    // A lone CR is content; an input ending in LF has no empty last line.
    let lone = b"A=1\rB\r\n\n";
    let lines = scan_lines(&mut context, lone).expect("scan");
    assert_eq!(lines.lines().len(), 2);
    assert_eq!(lines.content(&lines.lines()[0]), b"A=1\rB");
    assert_eq!(lines.content(&lines.lines()[1]), b"");
    assert_eq!(lines.lines()[1].terminator, LineTerminator::Lf);
    assert_eq!(lines.reassemble(), lone);
    assert!(
        scan_lines(&mut context, b"")
            .expect("scan")
            .lines()
            .is_empty()
    );
}

/// AC01: non-ASCII names are neither rejected nor transcoded.
#[test]
fn accept_f12_a_non_ascii_names_survive_as_bytes() {
    let list = read(FIXTURE);
    let keys: Vec<_> = list.entries().map(|(_, _, entry)| entry.key).collect();
    assert!(keys.contains(&&b"Caf\xe9"[..]), "Windows-1252 name");
    assert!(keys.contains(&&b"Z\xc3\xa9ro"[..]), "UTF-8 name");
    let section = b"[\xc4\xe4]\r\n\xff=\x80\r\n";
    let list = read(section);
    assert_eq!(
        list.lines()[0].kind,
        LineKind::Section { name: b"\xc4\xe4" }
    );
    assert_eq!(entry(&list, b"\xff").value, b"\x80");
    assert_eq!(list.reassemble(), section);
}

/// Quoting the survey never observed is kept raw, not guessed.
#[test]
fn accept_f12_a_unobserved_quoting_is_unsplit_not_guessed() {
    let cases: [(&[u8], QuoteIssue, usize); 4] = [
        (b"K=a,\"b,c\r\n", QuoteIssue::Unterminated, 2),
        (b"K=\"a\"b,c\r\n", QuoteIssue::TextAfterClosingQuote, 2),
        (b"K=\"a\"\"b\"\r\n", QuoteIssue::TextAfterClosingQuote, 2),
        (b"K=a,b\"c\"\r\n", QuoteIssue::QuoteInsideField, 3),
    ];
    for (bytes, issue, at) in cases {
        let list = read(bytes);
        let key = entry(&list, b"K");
        assert_eq!(key.fields, Fields::Unsplit { issue, at }, "{bytes:?}");
        assert_eq!(key.value, &bytes[2..bytes.len() - 2]);
        assert_eq!(list.reassemble(), bytes);
    }
    assert_eq!(QuoteIssue::Unterminated.code(), "unterminated_quote");
}

/// Lines no observed rule explains are kept and counted.
#[test]
fn accept_f12_a_unclassified_lines_are_kept_and_counted() {
    let list = read(FIXTURE);
    let unclassified: Vec<_> = list.unclassified().collect();
    assert_eq!(unclassified.len(), 1);
    assert_eq!(unclassified[0].content, b":not an entry");
    assert_eq!(
        unclassified[0].kind,
        LineKind::Unclassified {
            reason: Unclassified::NoSeparator
        }
    );

    let odd = b" =empty key\r\n[open\r\n[a]b\r\n";
    let list = read(odd);
    let reasons: Vec<_> = list
        .unclassified()
        .map(|line| match line.kind {
            LineKind::Unclassified { reason } => reason.code(),
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(
        reasons,
        vec!["empty_key", "malformed_section", "malformed_section"]
    );
    assert_eq!(list.entries().count(), 0);
    assert_eq!(list.reassemble(), odd);
}

/// The line and node tables are booked; a refused read books nothing.
#[test]
fn accept_f12_a_node_tables_are_bounded_by_the_allocation_budget() {
    let mut context = ParseContext::new("tiny.list", 64, 8);
    let error = read_keyed_list(&mut context, FIXTURE).expect_err("budget too small");
    assert_eq!(error.kind, ParseErrorKind::AllocationBudgetExceeded);
    assert_eq!(context.allocation().used(), 0);

    let mut context = ParseContext::with_defaults("fixture.list");
    read_keyed_list(&mut context, FIXTURE).expect("reads");
    assert!(context.allocation().used() > 0);
}

/// Members route by observed rule, never by extension alone.
#[test]
fn accept_f12_a_dialect_inventory_routes_only_observed_members() {
    assert_eq!(
        dialect_for_member(CRIMSON_ROF, Some("ASSETS/LAYOUT.CSV")),
        Some(TextDialect::KeyedList)
    );
    assert_eq!(
        dialect_for_member("gosdata/assets/CRIMSON.ROF", Some("assets/scrapbook.csv")),
        Some(TextDialect::KeyedList)
    );
    assert_eq!(
        dialect_for_member(CRIMSON_ROF, Some("ASSETS/OTHER.CSV")),
        None
    );
    assert_eq!(
        dialect_for_member("other.rof", Some("ASSETS/LAYOUT.CSV")),
        None
    );
    assert_eq!(dialect_for_member("LAYOUT.CSV", None), None);
    assert_eq!(
        dialect_for_member(CRIMSON_ROF, Some("ASSETS/SCRIPTS/MAINMENU.SCRIPT")),
        Some(TextDialect::UiScript)
    );
    assert_eq!(
        dialect_for_member(CRIMSON_ROF, Some("ASSETS/SCRIPTS/SUB/X.SCRIPT")),
        None
    );
    assert_eq!(
        dialect_for_member(CRIMSON_ROF, Some("ASSETS/SCRIPTS/.SCRIPT")),
        None
    );
    assert_eq!(
        dialect_for_member("STRINGS.DLL", None),
        Some(TextDialect::PeResources)
    );
    assert_eq!(
        dialect_for_member(CRIMSON_ROF, Some("ASSETS/SCRIPTS/\u{e9}")),
        None
    );

    // One row per dialect; only the keyed list has a reader here, and no
    // row claims more than a tool observation unless its format is public.
    assert_eq!(TEXT_DIALECT_INVENTORY.len(), TextDialect::ALL.len());
    for dialect in TextDialect::ALL {
        let record = dialect.record();
        assert!(!record.members.is_empty(), "{}", dialect.code());
        assert!(!record.unknowns.is_empty(), "{}", dialect.code());
        assert_ne!(record.grammar, ClaimStatus::VerifiedOriginal);
        assert_eq!(
            matches!(record.reader, DialectReader::KeyedList),
            dialect == TextDialect::KeyedList
        );
    }
    assert_eq!(TextDialect::UiScript.record().grammar, ClaimStatus::Unknown);
    assert_eq!(
        TextDialect::KeyedList.record().terminator,
        Some(LineTerminator::CrLf)
    );
}

/// Reads one directory block of `rof` and returns its entries as
/// `(name, record)`, NUL terminators removed.
fn rof_entries(rof: &[u8], offset: u32) -> Vec<(String, crate::RofRawRecord)> {
    let mut context = ParseContext::with_defaults(CRIMSON_ROF);
    let block = crate::read_directory(&mut context, &rof[offset as usize..])
        .expect("a retail directory block reads");
    block
        .entries()
        .map(|entry| {
            let name = entry.name.strip_suffix(&[0]).unwrap_or(entry.name);
            (String::from_utf8_lossy(name).into_owned(), entry.record)
        })
        .collect()
}

/// Walks `crimson.rof` one directory block at a time down `path` and reads
/// the member through the production bounded reader. (`read_tree` refuses
/// the retail archive for its overlapping extents, which F05-D owns.)
fn retail_member(rof: &[u8], path: &str) -> Vec<u8> {
    let mut offset = 0u32;
    let segments: Vec<&str> = path.split('/').collect();
    for (depth, segment) in segments.iter().enumerate() {
        let (_, record) = rof_entries(rof, offset)
            .into_iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(segment))
            .unwrap_or_else(|| panic!("{path}: no `{segment}` in the retail archive"));
        if depth + 1 < segments.len() {
            assert!(
                record.flags.is_directory(),
                "{path}: {segment} is a directory"
            );
            offset = record.start;
            continue;
        }
        let member = crate::RofMember {
            path: Vec::new(),
            record,
            start: u64::from(record.start),
            length_end: u64::from(record.start) + u64::from(record.raw_length),
            length_on_disk_end: u64::from(record.start) + u64::from(record.raw_length_on_disk),
        };
        let context = ParseContext::with_defaults(CRIMSON_ROF);
        return crate::read_member(&context, rof, &member, &crate::RofLimits::default())
            .expect("a retail member reads")
            .data;
    }
    unreachable!("paths have at least one segment")
}

fn retail_rof() -> Vec<u8> {
    let dir = std::env::var_os("CS_GAME_DIR")
        .expect("CS_GAME_DIR is not set: this test needs the original installation");
    let path = std::path::PathBuf::from(dir).join(CRIMSON_ROF);
    std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The inventory's keyed list claims hold for the retail members: length,
/// CRLF only, ASCII only, every quoted value splits, byte-exact
/// reassembly, and exactly the one `:` line of `LAYOUT.CSV` unclassified.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f12_a_retail_keyed_lists_read_losslessly() {
    let rof = retail_rof();
    let mut quoted_fields = 0usize;
    for rule in TextDialect::KeyedList.record().members {
        let MemberRule::Member {
            container,
            member,
            length,
        } = *rule
        else {
            panic!("keyed list rows name members")
        };
        assert_eq!(container, CRIMSON_ROF);
        let bytes = retail_member(&rof, member);
        assert_eq!(bytes.len() as u64, length, "{member}");
        assert_eq!(
            dialect_for_member(container, Some(member)),
            Some(TextDialect::KeyedList)
        );
        assert!(bytes.is_ascii(), "{member}: the survey found ASCII only");

        let mut context = ParseContext::with_defaults(member);
        let list = read_keyed_list(&mut context, &bytes).expect("reads");
        assert_eq!(list.reassemble(), bytes, "{member}");
        assert!(
            list.lines()
                .iter()
                .all(|line| line.terminator() == LineTerminator::CrLf),
            "{member}: CRLF only"
        );
        let unclassified: Vec<_> = list
            .unclassified()
            .map(|line| (line.line.number, line.content.first().copied()))
            .collect();
        let expected_unclassified = usize::from(member.ends_with("LAYOUT.CSV"));
        assert_eq!(
            unclassified.len(),
            expected_unclassified,
            "{member}: {unclassified:?}"
        );
        assert!(unclassified.iter().all(|(_, first)| *first == Some(b':')));
        assert!(list.entries().count() > 0);
        for (_, _, entry) in list.entries() {
            match &entry.fields {
                Fields::Split(fields) => {
                    quoted_fields += fields.iter().filter(|field| field.quoted).count();
                }
                Fields::Unsplit { issue, at } => {
                    panic!("{member}: {} at {at} in {:?}", issue.code(), entry.key)
                }
            }
        }
    }
    assert!(quoted_fields > 0, "the survey found quoted fields");
}

/// The other text rows: lengths and terminators as recorded, and the
/// `.SCRIPT` directory rule covering exactly the surveyed count.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f12_a_retail_text_inventory_matches_the_survey() {
    let rof = retail_rof();
    for dialect in [TextDialect::ResourceHeader, TextDialect::SymbolMap] {
        let record = dialect.record();
        for rule in record.members {
            let MemberRule::Member { member, length, .. } = *rule else {
                panic!("{} rows name members", dialect.code())
            };
            let bytes = retail_member(&rof, member);
            assert_eq!(bytes.len() as u64, length, "{member}");
            let mut context = ParseContext::with_defaults(member);
            let lines = scan_lines(&mut context, &bytes).expect("scan");
            assert_eq!(lines.reassemble(), bytes);
            let counts = lines.terminator_counts();
            match record.terminator {
                Some(LineTerminator::CrLf) => assert_eq!((counts.lf, counts.none), (0, 0)),
                Some(LineTerminator::Lf) => assert_eq!(counts.crlf, 0),
                other => panic!("{member}: {other:?}"),
            }
        }
    }

    let MemberRule::Directory {
        directory,
        suffix,
        count,
        ..
    } = TextDialect::UiScript.record().members[0]
    else {
        panic!("the script row is a directory rule")
    };
    let scripts_dir = directory.trim_end_matches('/');
    let assets = rof_entries(&rof, 0)
        .into_iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("ASSETS"))
        .expect("ASSETS")
        .1;
    let scripts = rof_entries(&rof, assets.start)
        .into_iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(&scripts_dir["ASSETS/".len()..]))
        .expect("SCRIPTS")
        .1;
    let routed = rof_entries(&rof, scripts.start)
        .into_iter()
        .filter(|(name, record)| {
            !record.flags.is_directory()
                && dialect_for_member(CRIMSON_ROF, Some(&format!("{directory}{name}")))
                    == Some(TextDialect::UiScript)
        })
        .count();
    assert_eq!(routed as u32, count);
    assert!(suffix.starts_with('.'));
}
