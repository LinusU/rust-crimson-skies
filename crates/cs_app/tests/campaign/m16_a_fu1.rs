//! Acceptance stage M16-A-FU1: the title source span is the row's own bytes,
//! not the `RT_STRING` block it was decoded from (`missions/M16.md`, work
//! order `M16-A`; follow-up #478 raised by the M16-A review).
//!
//! [`SourceContext::bind`] used to record the confirmed title's source span
//! from [`StringRow::span`], which locates the whole PE `RT_STRING` *block*
//! — up to sixteen unrelated strings — not the row. On the retail
//! installation that meant an M16 title span of 876 bytes covering the short
//! names of M09..M24, and no digest pinned the row either (the record's
//! `sha256` is the whole-asset digest).
//!
//! The stage's minimum scenario is that the cited span is the matched row's
//! own bytes:
//!
//! * [`accept_m16_a_fu1_the_title_span_is_the_matched_rows_own_bytes`]
//!   re-measures the row's own byte range from the decoded units and asserts
//!   the cited span equals it, decodes the cited bytes back to the row's code
//!   units, and shows no other campaign mission's title row overlaps it.
//! * [`accept_m16_a_fu1_the_enclosure_is_kept_distinct_from_the_cited_span`]
//!   pins that the `RT_STRING` block survives as the named
//!   [`SourceBinding::title_enclosure`] and is strictly larger than the cited
//!   row.
//!
//! Both tests read `$CS_GAME_DIR` through production code and are
//! `#[ignore = "requires CS_GAME_DIR"]`, so CI (which has no original data)
//! skips them and the implementing and reviewing agents run them with
//! `--include-ignored`. They fail if the span fix is reverted: a reverted
//! binding cites the 876-byte block, which is neither contained in the row's
//! measured range nor equal to the row's decoded bytes.

use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use cs_content::campaign_bindings::{MissionLabel, SourceBinding, SourceContext};
use cs_content::config::StringRow;
use cs_types::asset_id::SourceSpan;

use crate::common::load_inventory;

/// The one work order this stage binds.
const WORK_ORDER: &str = "M16";

/// The original installation, as the environment declares it.
fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M16-A-FU1 needs the retail capability; run this suite with \
             `--include-ignored` and CS_GAME_DIR pointing at the read-only installation"
        )
    }))
}

/// The source context, read once for the whole suite.
fn context() -> &'static SourceContext {
    static CONTEXT: OnceLock<SourceContext> = OnceLock::new();
    CONTEXT.get_or_init(|| {
        SourceContext::read(&game_dir()).expect("the installation yields a source context")
    })
}

/// The declared discovery title of `M16`, read from the committed inventory
/// rather than repeated here.
fn discovery_title() -> String {
    load_inventory()
        .iter()
        .find(|(label, _)| label.as_str() == WORK_ORDER)
        .map(|(_, title)| title.clone())
        .unwrap_or_else(|| panic!("the declared inventory has no {WORK_ORDER} work order"))
}

/// The M16 binding derived from the installation, built once.
fn binding() -> &'static SourceBinding {
    static BINDING: OnceLock<SourceBinding> = OnceLock::new();
    BINDING.get_or_init(|| {
        context()
            .bind(
                MissionLabel::new(WORK_ORDER).expect("M16 is a valid label"),
                &discovery_title(),
            )
            .expect("M16 binds to the original data")
    })
}

/// The comparable text of one retail row: a leading display tag such as
/// `[AB14I]` is a presentation instruction, not part of the title.
fn display_text(text: &str) -> &str {
    let Some(rest) = text.strip_prefix('[') else {
        return text;
    };
    let Some(end) = rest.find(']') else {
        return text;
    };
    &text[end + 2..]
}

/// The row whose id is `id`, from the table the binding was derived from.
fn row(context: &SourceContext, id: u32) -> &StringRow {
    context
        .string_rows()
        .iter()
        .find(|row| row.id == id)
        .unwrap_or_else(|| panic!("the table has no row {id}"))
}

/// The confirmed title row: the one M16's own binding resolved.
fn confirmed_row() -> &'static StringRow {
    let id = binding()
        .localized_title_id
        .expect("M16's title string resolved");
    row(context(), id)
}

/// The byte range the confirmed row itself occupies inside its asset,
/// re-measured here from the decoded units rather than trusting the
/// production helper.
///
/// [`StringRow::span`] is the block; the row's own range is the block's start
/// plus the encoded lengths (`2 + 2 * code_units.len()`) of every earlier row
/// in the same block. Rows of one block share the block span, and
/// [`SourceContext::string_rows`] lists them in block-then-unit order, so the
/// arithmetic is a re-derivation over the table the reader already decoded.
fn measured_row_range(context: &SourceContext, row: &StringRow) -> (u64, u64) {
    let mut offset = row.span.offset();
    for candidate in context.string_rows() {
        if candidate.span != row.span {
            continue;
        }
        let length = 2 + 2 * candidate.code_units.len() as u64;
        if candidate.id == row.id {
            return (offset, length);
        }
        offset += length;
    }
    panic!("row {} is not in the block it reports", row.id);
}

/// The span reduced to the two numbers a reader can check against bytes.
fn range(span: &SourceSpan) -> (u64, u64) {
    (span.offset(), span.length())
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m16_a_fu1_the_title_span_is_the_matched_rows_own_bytes() {
    let context = context();
    let binding = binding();
    let confirmed = confirmed_row();

    // The cited span is the row's own range, and it is not the whole block.
    let cited = binding
        .title_source
        .as_ref()
        .expect("a confirmed title has a source span");
    let (row_offset, row_length) = measured_row_range(context, confirmed);
    assert_eq!(
        range(cited),
        (row_offset, row_length),
        "the cited title span is not the confirmed row's own measured range"
    );
    assert!(
        cited.length() < confirmed.span.length(),
        "the cited title span ({}) is still the whole {}-byte RT_STRING block",
        cited.length(),
        confirmed.span.length()
    );

    // The cited bytes decode, unit for unit, to the confirmed row. This is the
    // check the old block span fails: a block span starts on a length prefix
    // and its decoded units are the block's, not the row's.
    let bytes = fs::read(game_dir().join(cited.container_path()))
        .unwrap_or_else(|error| panic!("cannot re-read {}: {error}", cited.container_path()));
    let start = usize::try_from(cited.offset()).expect("offset fits in usize");
    let end = usize::try_from(cited.offset() + cited.length()).expect("end fits in usize");
    assert!(
        end <= bytes.len(),
        "the cited span runs past the end of {}",
        cited.container_path()
    );
    let slice = &bytes[start..end];
    assert_eq!(slice.len(), 2 + 2 * confirmed.code_units.len());
    let count = u16::from_le_bytes([slice[0], slice[1]]) as usize;
    assert_eq!(
        count,
        confirmed.code_units.len(),
        "the cited bytes' code-unit count is not the confirmed row's"
    );
    let units: Vec<u16> = slice[2..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    assert_eq!(
        units, confirmed.code_units,
        "the cited bytes are not the confirmed row's code units"
    );

    // And what those bytes say is the discovery title this binding confirmed.
    let text = confirmed
        .text
        .as_deref()
        .expect("the confirmed row decodes");
    assert_eq!(
        display_text(text),
        discovery_title(),
        "the cited row does not carry the discovery title this binding confirmed"
    );

    // No other campaign mission's title row overlaps the cited range. The
    // installation carries two campaign-length runs — the region-prefixed long
    // names and the bare short names — and every row of both is checked
    // against the cited span, so the claim is about bytes and not about "the
    // M16 row" agreeing with itself.
    let cited_range = range(cited);
    let mut checked = 0usize;
    for block in context.campaign_title_blocks() {
        for id in block.first_id()..=block.last_id() {
            let other = row(context, id);
            if other.id == confirmed.id {
                continue;
            }
            let (other_offset, other_length) = measured_row_range(context, other);
            let overlaps = cited_range.0 < other_offset + other_length
                && other_offset < cited_range.0 + cited_range.1;
            assert!(
                !overlaps,
                "the cited title span {cited_range:?} overlaps row {} ({other_offset}+{other_length})",
                other.id
            );
            checked += 1;
        }
    }
    assert!(
        checked >= 2 * context.campaign().len() - 1,
        "expected the two campaign-length title runs to be checked, compared only {checked} rows"
    );

    // The installation spells M16's long name `Rocky Mountains - Raid on the
    // Rocky Express`; the join confirmed the *short* row this binding cites,
    // because a verbatim row wins over a region-prefixed long name. The long
    // name is a different row, outside the cited span. (The M16-A-FU1 task
    // description names the long name as "the row the join actually matched";
    // the measurement says otherwise and the correction is recorded in
    // `docs/findings/2026-10-02-m16-a-fu1-title-row-span.md`.)
    let long_name = context
        .string_rows()
        .iter()
        .find(|other| {
            other.id != confirmed.id
                && other
                    .text
                    .as_deref()
                    .map(display_text)
                    .is_some_and(|display| {
                        display.ends_with(&format!(" - {title}", title = discovery_title()))
                    })
        })
        .expect("the installation carries a region-prefixed long name for M16");
    assert_ne!(
        long_name.id, confirmed.id,
        "the short name and the long name are the same row, so the two forms are not independent"
    );
    assert!(
        long_name
            .text
            .as_deref()
            .map(display_text)
            .is_some_and(|display| display.contains(" - ")),
        "the second spelling is not a region-prefixed long name"
    );
    let (long_offset, long_length) = measured_row_range(context, long_name);
    assert!(
        !(cited_range.0 < long_offset + long_length && long_offset < cited_range.0 + cited_range.1),
        "the cited title span {} overlaps the long-name row {}",
        cited_range.0 + cited_range.1,
        long_name.id
    );
    assert!(
        long_offset + long_length <= cited.offset()
            || cited.offset() + cited.length() <= long_offset,
        "the long-name row and the cited span are not disjoint"
    );
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m16_a_fu1_the_enclosure_is_kept_distinct_from_the_cited_span() {
    // The block is not discarded: it survives as `title_enclosure`, clearly
    // named and strictly larger than the row, so a reader can still see which
    // `RT_STRING` block the string was decoded from without mistaking the
    // block for the title.
    let binding = binding();
    let cited = binding
        .title_source
        .as_ref()
        .expect("a confirmed title has a source span");
    let enclosure = binding
        .title_enclosure
        .as_ref()
        .expect("a confirmed title keeps the block it was decoded from");

    assert_eq!(
        enclosure,
        &confirmed_row().span,
        "the title enclosure is not the block the confirmed row reports"
    );
    assert_ne!(
        cited, enclosure,
        "the title span is still the whole enclosing RT_STRING block"
    );
    assert!(
        cited.offset() >= enclosure.offset()
            && cited.offset() + cited.length() <= enclosure.offset() + enclosure.length(),
        "the cited title span {cited} is not inside the enclosure it was decoded from {enclosure}"
    );
    assert!(
        cited.length() < enclosure.length(),
        "the enclosure is not larger than the row it encloses"
    );
}
