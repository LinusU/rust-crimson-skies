//! `accept_f12_b_*` for the `#define` resource-id header reader
//! (`crate::text::resource_header`).
//!
//! The fixture is **newly authored**: it follows the shape the survey found
//! in `RESOURCE.H` and `RESRC1.H` (a VS dependency marker, a licence comment,
//! an include guard, `#define NAME <decimal>` lines, blank lines) but
//! reproduces no identifier, no value and no comment of the original headers.
//! The one test marked `#[ignore = "requires CS_GAME_DIR"]` checks the
//! recorded structural facts of the real members instead.

use std::ops::Range;

use cs_types::evidence::ClaimStatus;

use super::dialect::{
    CRIMSON_ROF, DialectReader, MemberRule, TEXT_DIALECT_INVENTORY, TextDialect, dialect_for_member,
};
use super::resource_header::{
    HeaderLookup, MAX_RESOURCE_ID, RESOURCE_HEADER_ENTRYPOINT, ResourceHeader, ResourceHeaderKind,
    ResourceHeaderLine, read_resource_header,
};
use super::tests::{retail_member, retail_rof};
use super::{LineTerminator, TextLine};
use crate::pe_resources::{LANG_ENGLISH_US, PE_RESOURCES_ENTRYPOINT, RT_STRING, string_id};
use crate::{ParseContext, ParseErrorKind};

/// A resource header shaped like the surveyed ones: a VS dependency marker, a
/// comment, a blank line, defines, an include guard and the APS trailer.
const HEADER: &[u8] = b"//{{NO_DEPENDENCIES}}\r\n\
// resource ids\r\n\
\r\n\
#define APP_ICON 9\r\n\
#define IDD_DIALOG1 1033\r\n\
#define IDB_TILE0\r\n\
#define IDS_CANCEL 251\r\n\
\r\n\
#define _APS_NEXT_RESOURCE_VALUE 40202\r\n\
#define _APS_NEXT_COMMAND_VALUE 4000\r\n\
#ifndef __MIT_H__\r\n\
#define __MIT_H__\r\n\
#endif\r\n";

fn read(bytes: &[u8]) -> ResourceHeader<'_> {
    let mut context = ParseContext::with_defaults(CRIMSON_ROF);
    read_resource_header(&mut context, bytes).expect("an authored header reads")
}

/// The reader keeps every line and every byte, splits `#define` lines into a
/// name and a raw value, and never interprets a value whose shape the survey
/// did not observe.
#[test]
fn accept_f12_b_resource_header_keeps_every_line_and_raw_values() {
    let header = read(HEADER);
    assert_eq!(header.reassemble(), HEADER);
    assert_eq!(header.bytes(), HEADER);
    assert_eq!(header.lines().len(), 13);
    assert_eq!(header.defines().count(), 7);
    assert_eq!(header.unclassified().count(), 0);

    // Line terminators and offsets survive.
    assert!(
        header
            .lines()
            .iter()
            .all(|line| line.terminator() == LineTerminator::CrLf)
    );
    for (index, line) in header.lines().iter().enumerate() {
        assert_eq!(line.line.number, index as u64 + 1);
        assert_eq!(
            line.content,
            &HEADER[line.line.offset as usize..][..line.content.len()]
        );
    }

    // Only the four observed line shapes appear, and each one's own bytes are
    // kept.
    let kinds: Vec<&ResourceHeaderKind<'_>> =
        header.lines().iter().map(|line| &line.kind).collect();
    assert!(matches!(kinds[0], ResourceHeaderKind::Comment { .. }));
    assert_eq!(
        match kinds[0] {
            ResourceHeaderKind::Comment { text } => *text,
            _ => &b""[..],
        },
        &b"{{NO_DEPENDENCIES}}"[..]
    );
    assert!(matches!(kinds[2], ResourceHeaderKind::Blank));
    assert!(matches!(kinds[3], ResourceHeaderKind::Define { .. }));
    assert_eq!(
        match kinds[10] {
            ResourceHeaderKind::Directive { name, rest } => (*name, *rest),
            _ => (&b""[..], &b""[..]),
        },
        (&b"ifndef"[..], &b" __MIT_H__"[..])
    );
    assert!(matches!(kinds[12], ResourceHeaderKind::Directive { .. }));

    // Every define keeps its name, its raw value, its line and its ranges.
    /// `(name, value text, line, name range, value range)` per `#define`.
    type DefineRow<'a> = (&'a [u8], &'a [u8], u64, Range<usize>, Range<usize>);
    let rows: Vec<DefineRow<'_>> = header
        .defines()
        .map(|define| {
            (
                define.name,
                define.value.text(),
                define.line,
                define.name_range.clone(),
                define.value.range.clone(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            (&b"APP_ICON"[..], &b"9"[..], 4, 8..16, 16..18),
            (&b"IDD_DIALOG1"[..], &b"1033"[..], 5, 8..19, 19..24),
            (&b"IDB_TILE0"[..], &b""[..], 6, 8..17, 17..17),
            (&b"IDS_CANCEL"[..], &b"251"[..], 7, 8..18, 18..22),
            (
                &b"_APS_NEXT_RESOURCE_VALUE"[..],
                &b"40202"[..],
                9,
                8..32,
                32..38
            ),
            (
                &b"_APS_NEXT_COMMAND_VALUE"[..],
                &b"4000"[..],
                10,
                8..31,
                31..36
            ),
            (&b"__MIT_H__"[..], &b""[..], 12, 8..17, 17..17),
        ]
    );

    // The value's raw bytes keep the original spelling; `text()` trims only
    // the blank bytes around the number.
    let icon = header.define(b"APP_ICON").expect("the define");
    assert_eq!(icon.value.raw, b" 9", "the raw value keeps its separator");
    assert_eq!(icon.value.text(), b"9");
    assert_eq!(icon.name_range, 8..16);
    let spaced = read(b"#define PADDED   \t 42  \r\n");
    assert_eq!(spaced.define(b"PADDED").unwrap().value.raw, b"   \t 42  ");
    assert_eq!(spaced.define(b"PADDED").unwrap().value.text(), b"42");
    assert_eq!(spaced.reassemble(), b"#define PADDED   \t 42  \r\n");

    // The one value shape the reader interprets: a plain decimal that is a
    // resource id inside the 16-bit id space the format addresses.
    assert_eq!(icon.resource_id(), Some(9));
    assert_eq!(
        header.define(b"IDD_DIALOG1").unwrap().resource_id(),
        Some(1033)
    );
    assert_eq!(
        header
            .define(b"_APS_NEXT_RESOURCE_VALUE")
            .unwrap()
            .resource_id(),
        Some(40202)
    );
    // A define with no value is an object-like macro, not a resource id.
    assert_eq!(header.define(b"IDB_TILE0").unwrap().resource_id(), None);
    assert_eq!(header.define(b"__MIT_H__").unwrap().resource_id(), None);
    assert_eq!(header.define(b"ABSENT"), None);
    // A duplicate name is an ambiguous lookup, never a silent overwrite.
    let duplicated = read(b"#define A 1\r\n#define A 2\r\n");
    assert_eq!(duplicated.defines().count(), 2);
    assert!(matches!(
        duplicated.lookup(b"A"),
        HeaderLookup::Ambiguous(2)
    ));
    assert_eq!(duplicated.define(b"A"), None);
    assert!(matches!(duplicated.lookup(b"B"), HeaderLookup::Missing));
    let single = read(b"#define A 1\r\n#define B 2\r\n");
    assert!(matches!(single.lookup(b"A"), HeaderLookup::Found(_)));
}

/// A value that is not a decimal resource id — hexadecimal, an expression, a
/// negative number, a number too wide for the id space, a spelling with
/// leading zeros — keeps its raw bytes and never becomes an id.
#[test]
fn accept_f12_b_resource_header_values_are_typed_and_checked() {
    const CASES: &[(&str, &str, Option<u32>)] = &[
        ("#define A 0x10\r\n", "0x10", None),
        ("#define B 65536\r\n", "65536", None),
        ("#define C -1\r\n", "-1", None),
        ("#define D 1+2\r\n", "1+2", None),
        ("#define E 4294967296\r\n", "4294967296", None),
        (
            "#define F 99999999999999999999\r\n",
            "99999999999999999999",
            None,
        ),
        ("#define G 007\r\n", "007", Some(7)),
        ("#define H 65535\r\n", "65535", Some(65535)),
        ("#define I\r\n", "", None),
        ("#define\tJ\t9\r\n", "9", Some(9)),
    ];
    for (line, raw, expected) in CASES {
        let header = read(line.as_bytes());
        assert_eq!(header.unclassified().count(), 0, "{line}");
        let define = header
            .defines()
            .next()
            .unwrap_or_else(|| panic!("{line} classifies as a define"));
        assert_eq!(define.value.text(), raw.as_bytes(), "{line}");
        assert_eq!(define.value.is_decimal(), expected.is_some(), "{line}");
        assert_eq!(define.resource_id(), *expected, "{line}");
        assert_eq!(header.reassemble(), line.as_bytes(), "{line}");
    }
    // `MAX_RESOURCE_ID` is the format's own ceiling, and it is inclusive.
    assert_eq!(MAX_RESOURCE_ID, 65535);
    // A value with a trailing comment keeps every byte and names no id.
    let trailing = read(b"#define A 9 // ten\r\n");
    assert_eq!(trailing.define(b"A").unwrap().value.raw, b" 9 // ten");
    assert_eq!(trailing.define(b"A").unwrap().value.text(), b"9 // ten");
    assert_eq!(trailing.define(b"A").unwrap().resource_id(), None);
    // A line that is neither blank, comment, define nor directive is kept and
    // counted, not guessed at.
    let stray = read(b"nonsense\r\n");
    assert_eq!(stray.unclassified().count(), 1);
    assert_eq!(stray.defines().count(), 0);
    assert_eq!(stray.reassemble(), b"nonsense\r\n");
    // A single `/` is not a comment.
    assert_eq!(read(b"/ one\r\n").unclassified().count(), 1);
    // An LF-only input and a last line with no terminator are both read.
    let mixed = read(b"#define A 1\n#define B 2");
    assert_eq!(mixed.defines().count(), 2);
    assert_eq!(mixed.reassemble(), b"#define A 1\n#define B 2");
    assert_eq!(mixed.lines()[0].terminator(), LineTerminator::Lf);
    assert_eq!(mixed.lines()[1].terminator(), LineTerminator::None);
}

/// The line and node tables are booked against the parse's allocation budget:
/// the exact budget reads, one byte less is refused, and the refusal leaves
/// the ledger as it found it.
#[test]
fn accept_f12_b_resource_header_is_bounded_by_the_allocation_budget() {
    let mut probe = ParseContext::with_defaults(CRIMSON_ROF);
    let header = read_resource_header(&mut probe, HEADER).expect("reads");
    assert_eq!(header.reassemble(), HEADER);
    let needed = probe.allocation().used();
    let expected = HEADER.iter().filter(|byte| **byte == b'\n').count() as u64
        * (std::mem::size_of::<TextLine>() as u64
            + std::mem::size_of::<ResourceHeaderLine<'_>>() as u64);
    assert_eq!(needed, expected, "the reader books both tables exactly");

    let mut exact = ParseContext::new("exact", needed, 8);
    read_resource_header(&mut exact, HEADER).expect("exactly the budget it books is enough");
    assert_eq!(exact.allocation().used(), needed);

    let mut short = ParseContext::new("short", needed - 1, 8);
    let error = read_resource_header(&mut short, HEADER).expect_err("one byte short is refused");
    assert_eq!(error.kind, ParseErrorKind::AllocationBudgetExceeded);
    assert!(error.field.starts_with(RESOURCE_HEADER_ENTRYPOINT));
    assert_eq!(short.allocation().used(), 0, "a refusal books nothing");
}

/// The inventory rows this stage gained readers for no longer defer one, the
/// other rows are unchanged, and nothing here claims more evidence class than
/// the survey supports.
#[test]
fn accept_f12_b_dialect_inventory_points_at_the_new_readers() {
    for (dialect, reader) in [
        (TextDialect::ResourceHeader, DialectReader::ResourceHeader),
        (TextDialect::PeResources, DialectReader::PeResources),
    ] {
        let record = dialect.record();
        assert_eq!(record.dialect, dialect);
        assert_eq!(record.reader, reader, "{dialect:?} names the wrong reader");
        assert!(
            record.reader.entrypoint().is_some(),
            "{dialect:?} has no entrypoint"
        );
        assert!(
            record.grammar != ClaimStatus::VerifiedOriginal,
            "{:?} must not be claimed original-verified",
            dialect
        );
        assert!(matches!(
            record.grammar,
            ClaimStatus::ObservedTool | ClaimStatus::Documented
        ));
        assert!(
            !record.unknowns.is_empty(),
            "{:?} records unknowns",
            dialect
        );
    }
    for dialect in [
        TextDialect::UiScript,
        TextDialect::SymbolMap,
        TextDialect::RichText,
    ] {
        assert!(
            matches!(dialect.record().reader, DialectReader::Deferred { .. }),
            "{:?} must still be deferred",
            dialect
        );
    }
    assert_eq!(
        TextDialect::KeyedList.record().reader,
        DialectReader::KeyedList
    );
    assert_eq!(TextDialect::ALL.len(), TEXT_DIALECT_INVENTORY.len());
    // The two entrypoints this stage adds are named and distinct, and routing
    // still works through the observed member rules.
    assert_eq!(RESOURCE_HEADER_ENTRYPOINT, "text.resource_header");
    assert_eq!(PE_RESOURCES_ENTRYPOINT, "pe.resources");
    assert_eq!(
        dialect_for_member(CRIMSON_ROF, Some("ASSETS/SCRIPTS/RESOURCE.H")),
        Some(TextDialect::ResourceHeader)
    );
    assert_eq!(
        dialect_for_member("strings.dll", None),
        Some(TextDialect::PeResources)
    );
    // A resource id in a header and a string id in a PE block are the same
    // number space. Block ids are 1-based and each block carries sixteen ids,
    // so block 1 holds ids 0..=15 and block 2 holds 16..=31.
    assert_eq!(string_id(1, 0), 0);
    assert_eq!(string_id(1, 15), 15);
    assert_eq!(string_id(2, 0), 16);
    // The largest id a full block range can name. It exceeds the 16-bit id
    // space a `.H` `#define` can spell, so the two readers' ranges differ and
    // a consumer has to notice that rather than assume one space.
    assert_eq!(string_id(u16::MAX, 15), 1_048_559);
    assert!(string_id(u16::MAX, 15) > MAX_RESOURCE_ID);
    assert_eq!(RT_STRING, 6);
    assert_eq!(LANG_ENGLISH_US, 1033);
}

// ------------------------------------------------------------ retail checks

/// The recorded structural facts of the two surveyed `.H` members: length,
/// CRLF only, nothing unclassified, several hundred defines with unique
/// names, and every value a plain decimal inside the 16-bit resource-id
/// space.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f12_b_retail_resource_headers_read_within_the_id_space() {
    let rof = retail_rof();
    let members: Vec<(&str, u64)> = TEXT_DIALECT_INVENTORY
        .iter()
        .flat_map(|record| record.members.iter())
        .filter_map(|rule| match rule {
            MemberRule::Member { member, length, .. } => Some((*member, *length)),
            _ => None,
        })
        .filter(|(member, _)| {
            dialect_for_member(CRIMSON_ROF, Some(member)) == Some(TextDialect::ResourceHeader)
        })
        .collect();
    assert_eq!(members.len(), 2, "both surveyed resource headers");

    for (member, length) in members {
        let bytes = retail_member(&rof, member);
        assert_eq!(bytes.len() as u64, length, "{member}");

        let mut context = ParseContext::with_defaults(member);
        let header = read_resource_header(&mut context, &bytes)
            .unwrap_or_else(|error| panic!("{member}: {error}"));
        assert_eq!(header.reassemble(), bytes, "{member}");
        assert!(
            header
                .lines()
                .iter()
                .all(|line| line.terminator() == LineTerminator::CrLf),
            "{member}: CRLF only"
        );
        assert_eq!(header.unclassified().count(), 0, "{member}");

        // Every `#define` names a resource id the PE reader could be asked
        // for, and every name is unique so a lookup is never ambiguous.
        let defines: Vec<_> = header.defines().collect();
        assert!(defines.len() > 100, "{member}: {} defines", defines.len());
        let mut names: Vec<&[u8]> = Vec::with_capacity(defines.len());
        for define in &defines {
            let id = define.resource_id().unwrap_or_else(|| {
                panic!(
                    "{member} line {}: {:?} is not a resource id",
                    define.line,
                    define.value.text()
                )
            });
            assert!(id <= MAX_RESOURCE_ID, "{member}: id {id}");
            names.push(define.name);
        }
        names.sort_unstable();
        let unique = names.len();
        names.dedup();
        assert_eq!(names.len(), unique, "{member}: duplicate define names");
    }
}
