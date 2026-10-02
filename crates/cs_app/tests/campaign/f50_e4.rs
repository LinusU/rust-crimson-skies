//! Acceptance stage F50-E4: a retail comparison test for the row-geometry and
//! title-exactness rules of `cs_content::campaign_bindings`
//! (`specs/F50-per-mission-compatibility-and-full-campaign-closure.md`,
//! work order `F50-E4`, raised by the M18-A review of Rally #309).
//!
//! Two rules of [`SourceContext`] decide which localized rows may name a
//! campaign position, and every per-mission binding stage proved them only on
//! *authored* values, because no single retail installation contradicts them:
//!
//! 1. [`SourceContext::campaign_title_blocks`] — a row run must be exactly as
//!    long as the campaign the directory layout declares;
//! 2. [`title_form`] / [`SourceContext::confirm_title`] — a row carries a title
//!    only byte for byte, either as its whole display text or as the tail of a
//!    region-prefixed long name.
//!
//! The gap that cost M18-A its review: while verifying M18, mutating
//! `title_form` to accept a tail that merely *starts with* the title left all
//! eight `accept_m18_a_*` retail tests green, because the installation offers no
//! near miss a fuzzy comparison would take. The synthetic tests cover those
//! arms; nothing re-derived the *string table's own geometry* from the
//! installation. This stage closes that gap, and it closes it the only way that
//! can discriminate: by deriving the same facts a second time, here, from
//! `$CS_GAME_DIR`, sharing **no helper** with the code under test.
//!
//! * [`accept_f50_e4_the_row_runs_are_exactly_the_ones_an_independent_reading_finds`]
//!   re-derives the maximal runs of consecutive rows that carry display text,
//!   the campaign length from the `ZBD` directory layout and the runs that are
//!   as long as the campaign, then holds `campaign_title_blocks` to that set in
//!   both directions and re-derives the 48 campaign rows' own byte ranges out
//!   of the image.
//! * [`accept_f50_e4_the_confirmed_rows_are_exactly_the_exact_byte_matches`]
//!   re-derives, per row, the titles that row carries, and holds `title_form`
//!   (over every row) and `confirm_title` (over an authored near-miss set built
//!   from the installation and the committed inventory) to that inverted index.
//! * [`accept_f50_e4_a_fuzzy_matcher_would_confirm_a_near_miss_this_table_refuses`]
//!   shows the near misses are real: for three weakened matchers (a prefix, a
//!   substring and a case-insensitive comparison) it counts the candidate titles
//!   each *would* confirm out of this table and holds `confirm_title` to
//!   refusing every one of them.
//!
//! All three tests read `$CS_GAME_DIR` through production code and are
//! `#[ignore = "requires CS_GAME_DIR"]`, so CI (which has no original data)
//! skips them and the implementing and reviewing agents run them with
//! `--include-ignored`. Nothing here restates the implementation: every
//! expectation is either re-derived from the installation below, or a
//! measurement recorded in
//! `docs/findings/2026-10-03-f50-e4-row-geometry-and-title-exactness.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use cs_content::campaign_bindings::{SourceContext, TitleConfirmation, TitleForm, title_form};
use cs_content::config::StringRow;

use crate::common::load_inventory;

/// The localized UI string image this stage reads, as the installation spells
/// it.
const STRING_ASSET: &str = "GOSDATA/ASSETS/BINARIES/langui.dll";

/// The `" - "` separator of a region-prefixed long name, spelled here so that
/// this stage does not read it out of the code under test.
const SEPARATOR: &str = " - ";

// ---------------------------------------------------------------- inputs ---

/// The original installation, as the environment declares it.
fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: F50-E4 needs the retail capability; run this suite with \
             `--include-ignored` and CS_GAME_DIR pointing at the read-only installation"
        )
    }))
}

/// The source context, read once for the whole suite (fingerprinting the
/// installation walks every file, so it happens exactly once).
fn context() -> &'static SourceContext {
    static CONTEXT: OnceLock<SourceContext> = OnceLock::new();
    CONTEXT.get_or_init(|| {
        SourceContext::read(&game_dir()).expect("the installation yields a source context")
    })
}

/// The declared work-order titles, read from the committed inventory rather
/// than repeated here. Seven of the twenty-four are titles the installation
/// does not carry byte for byte, which is what makes them near misses.
fn declared_titles() -> Vec<(String, String)> {
    load_inventory()
        .iter()
        .map(|(label, title)| (label.as_str().to_owned(), title.clone()))
        .collect()
}

// ------------------------------------------ the independent reading only ---

/// The comparable text of one localized row, re-derived here: a leading display
/// tag such as `[AB14I]` is a presentation instruction and is not part of the
/// text, and a row that holds nothing comparable reads as empty.
///
/// Deliberately *not* `cs_content`'s private `strip_font_tag`, because the rules
/// under test are measured over this function's output. Where the two could
/// differ — a tag followed by more than one space — this one drops every space
/// and tab after the closing bracket, and the difference is measured rather than
/// assumed: this installation carries no row written either way, so both
/// readings name the same 1207 rows.
fn display(row: &StringRow) -> &str {
    let Some(text) = row.text.as_deref() else {
        return "";
    };
    match text.strip_prefix('[').and_then(|rest| rest.find(']')) {
        // `close` counts from the character after the opening bracket, so the
        // closing bracket itself sits at `close + 1`.
        Some(close) => text[close + 2..].trim_start_matches([' ', '\t']),
        None => text,
    }
}

/// The ids of the rows that carry display text, ascending and deduplicated.
///
/// This is the *present* set the row geometry is measured over: a row that
/// decodes to nothing ends a run, so a row with no text and a row whose id is
/// absent are the same fact for this purpose.
fn present_ids(rows: &[StringRow]) -> Vec<u32> {
    let mut ids: Vec<u32> = rows
        .iter()
        .filter(|row| !display(row).is_empty())
        .map(|row| row.id)
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// The maximal runs of consecutive ids in `ids`, ascending and disjoint.
///
/// A gap ends a run, and every run is maximal by construction: one is closed
/// only when the next id is not `last + 1`, or the input ended, so neither
/// neighbour of a run can extend it. Deliberately not
/// `cs_content::campaign_bindings::title_blocks`.
fn maximal_runs(ids: &[u32]) -> Vec<(u32, u32)> {
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for &id in ids {
        match runs.last_mut() {
            Some(last) if last.1 + 1 == id => last.1 = id,
            _ => runs.push((id, id)),
        }
    }
    runs
}

/// The width of one run.
fn width(run: (u32, u32)) -> usize {
    usize::try_from(run.1 - run.0 + 1).expect("a run's width fits in usize")
}

/// Whether row `id` carries comparable text of its own, read from the table
/// rather than from any reader of runs.
fn carries(rows: &[StringRow], id: u32) -> bool {
    rows.iter()
        .any(|row| row.id == id && !display(row).is_empty())
}

/// The display text of one row, by id.
fn row_display(rows: &[StringRow], id: u32) -> &str {
    display(
        rows.iter()
            .find(|row| row.id == id)
            .unwrap_or_else(|| panic!("the table has no row {id}")),
    )
}

/// The runs of `runs` that are exactly `campaign` rows long.
fn runs_of_campaign_length(runs: &[(u32, u32)], campaign: usize) -> Vec<(u32, u32)> {
    runs.iter()
        .copied()
        .filter(|run| width(*run) == campaign)
        .collect()
}

/// How one row carries one title, in this file's own terms.
///
/// Deliberately not [`TitleForm`]: the comparison maps this onto the production
/// enum only once both sides have decided, so a rule that drifted in production
/// cannot drift here with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Form {
    /// The row's whole display text is the title.
    Whole,
    /// The row is a region-prefixed long name and the part after its first
    /// separator is the title.
    Tail,
}

/// The title one display text carries, if any.
///
/// The comparison is byte equality in both arms, and the split is at the
/// *first* separator with a non-empty prefix and a non-empty remainder.
fn carried_by(display: &str, title: &str) -> Option<Form> {
    if display == title {
        return Some(Form::Whole);
    }
    let prefix_end = display.find(SEPARATOR)?;
    let (prefix, remainder) = display.split_at(prefix_end);
    let tail = &remainder[SEPARATOR.len()..];
    if prefix.is_empty() || tail.is_empty() {
        return None;
    }
    (tail == title).then_some(Form::Tail)
}

/// Every title the installation's rows carry, mapped to the rows that carry it
/// and how.
///
/// Built per *row* — each row contributes the titles it can carry — while
/// `cs_content` searches per *title* over the rows, so the two derivations share
/// no traversal, no helper and no direction.
fn carried_titles(rows: &[StringRow]) -> BTreeMap<String, Vec<(u32, Form)>> {
    let mut index: BTreeMap<String, Vec<(u32, Form)>> = BTreeMap::new();
    for row in rows {
        let text = display(row);
        if text.is_empty() {
            continue;
        }
        index
            .entry(text.to_owned())
            .or_default()
            .push((row.id, Form::Whole));
        if let Some(at) = text.find(SEPARATOR).filter(|at| *at > 0) {
            let tail = &text[at + SEPARATOR.len()..];
            if !tail.is_empty() {
                index
                    .entry(tail.to_owned())
                    .or_default()
                    .push((row.id, Form::Tail));
            }
        }
    }
    index
}

/// What an exact byte comparison alone says about one title.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Expected {
    /// Exactly one row carries it, in this form.
    Confirmed(u32, Form),
    /// Several rows carry it in the same form.
    Ambiguous {
        /// The rows that carry it.
        ids: Vec<u32>,
        /// The form they all carry it in.
        form: Form,
    },
    /// No row carries it.
    Uncarried,
}

/// The outcome [`carried_titles`] predicts for `title`: the verbatim rows win,
/// and the long names are consulted only when there is no verbatim row.
fn predict(index: &BTreeMap<String, Vec<(u32, Form)>>, title: &str) -> Expected {
    let carrying = index.get(title).map(Vec::as_slice).unwrap_or_default();
    for form in [Form::Whole, Form::Tail] {
        let ids: Vec<u32> = carrying
            .iter()
            .filter(|(_, carried)| *carried == form)
            .map(|(id, _)| *id)
            .collect();
        match ids.as_slice() {
            [only] => return Expected::Confirmed(*only, form),
            [] => {}
            _ => return Expected::Ambiguous { ids, form },
        }
    }
    Expected::Uncarried
}

/// This file's form as production spells it.
fn as_production(form: Form) -> TitleForm {
    match form {
        Form::Whole => TitleForm::Verbatim,
        Form::Tail => TitleForm::RegionPrefixedLongName,
    }
}

/// The production form, as this file's own two-armed name.
fn from_production(form: TitleForm) -> Form {
    match form {
        TitleForm::Verbatim => Form::Whole,
        TitleForm::RegionPrefixedLongName => Form::Tail,
    }
}

/// The campaign length and its per-chapter mission counts, re-derived from the
/// installation's directory layout instead of read from
/// [`SourceContext::campaign`].
///
/// A chapter is a `ZBD/C<digits><letters>` directory and a mission is an
/// `M<digits>` directory inside one: the layout `scan_campaign` walks, written
/// again here, because "as long as the campaign the directory layout declares"
/// is only a claim if the campaign length is measured too.
fn directory_campaign() -> (usize, Vec<usize>) {
    let zbd = fs::read_dir(game_dir())
        .unwrap_or_else(|error| panic!("cannot read the installation root: {error}"))
        .map(|entry| entry.expect("a directory entry").path())
        .find(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("ZBD"))
        })
        .expect("the installation has a ZBD container directory");
    let mut chapters: BTreeMap<u32, usize> = BTreeMap::new();
    for chapter in fs::read_dir(&zbd)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", zbd.display()))
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| path.is_dir())
    {
        let name = chapter
            .file_name()
            .expect("a chapter directory has a name")
            .to_string_lossy()
            .into_owned();
        let Some(number) = chapter_number(&name) else {
            continue;
        };
        let missions = fs::read_dir(&chapter)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", chapter.display()))
            .filter(|entry| {
                entry
                    .as_ref()
                    .map(|entry| entry.path().is_dir())
                    .unwrap_or(false)
            })
            .filter(|entry| {
                entry
                    .as_ref()
                    .ok()
                    .map(|entry| entry.file_name())
                    .is_some_and(|name| {
                        let name = name.to_string_lossy();
                        name.len() > 1
                            && name.starts_with('M')
                            && name[1..].chars().all(|c| c.is_ascii_digit())
                    })
            })
            .count();
        *chapters.entry(number).or_default() += missions;
    }
    assert!(
        !chapters.is_empty(),
        "the installation declares no campaign chapter directories under {}",
        zbd.display()
    );
    let sizes: Vec<usize> = chapters.values().copied().collect();
    (sizes.iter().sum(), sizes)
}

/// The chapter number of a `C<digits><letters>` directory name.
fn chapter_number(name: &str) -> Option<u32> {
    let rest = name.strip_prefix('C').or_else(|| name.strip_prefix('c'))?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    let suffix = &rest[digits.len()..];
    if !suffix.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    digits.parse().ok()
}

/// The edit distance between two strings, in characters, so the "the table
/// carries something close" arm below is a measurement and not an assertion
/// about a guess.
fn edit_distance(left: &str, right: &str) -> usize {
    let mut previous: Vec<usize> = (0..=right.chars().count()).collect();
    for (row_index, left_char) in left.chars().enumerate() {
        let mut current = vec![row_index + 1];
        for (column, right_char) in right.chars().enumerate() {
            current.push(
                (previous[column + 1] + 1)
                    .min(current[column] + 1)
                    .min(previous[column] + usize::from(left_char != right_char)),
            );
        }
        previous = current;
    }
    previous[right.chars().count()]
}

/// Every row id of `runs`, in run order and then row order.
fn campaign_rows(runs: &[(u32, u32)]) -> Vec<u32> {
    runs.iter()
        .flat_map(|&(first, last)| first..=last)
        .collect()
}

/// The authored near-miss titles. Every one is derived from the installation's
/// own campaign rows or from the committed inventory, and each is a title only
/// an exact comparison can agree with.
fn near_miss_titles(rows: &[StringRow], runs: &[(u32, u32)]) -> Vec<String> {
    let mut titles: Vec<String> = declared_titles()
        .into_iter()
        .map(|(_, title)| title)
        .collect();
    // The first campaign run is the region-prefixed long names, the second the
    // bare short names; `the_two_campaign_runs` asserts both facts.
    for (run_index, &(first, last)) in runs.iter().enumerate() {
        for id in first..=last {
            let text = row_display(rows, id);
            let separator = text.find(SEPARATOR).filter(|at| *at > 0);
            match (run_index == 0, separator) {
                // A long name confirms its own tail, and nothing that merely
                // starts with it, is re-cased, gains a space or gains a stop.
                (true, Some(at)) => {
                    let tail = &text[at + SEPARATOR.len()..];
                    titles.push(tail.to_owned());
                    titles.push(tail[..tail.len() - 1].to_owned());
                    titles.push(format!("{tail}."));
                    titles.push(format!(" {tail}"));
                    titles.push(tail.to_lowercase());
                }
                // The same four near misses against the verbatim arm.
                (false, None) => {
                    titles.push(text.to_owned());
                    titles.push(text[..text.len() - 1].to_owned());
                    titles.push(format!("{text} "));
                    titles.push(text.to_uppercase());
                }
                _ => {}
            }
        }
    }
    // The region prefixes of the long-name run: title-shaped strings the table
    // carries on their own, several of them more than once.
    for id in runs[0].0..=runs[0].1 {
        let text = row_display(rows, id);
        if let Some(at) = text.find(SEPARATOR).filter(|at| *at > 0) {
            titles.push(text[..at].to_owned());
        }
    }
    // The shortest row carrying two separators, which splits the two arms apart:
    // the text after the *first* separator is a tail, and neither of the two
    // segments after it is.
    let doubled = rows
        .iter()
        .filter(|row| display(row).matches(SEPARATOR).count() > 1)
        .min_by_key(|row| display(row).chars().count())
        .map(|row| row.id);
    if let Some(id) = doubled {
        let segments: Vec<&str> = row_display(rows, id).split(SEPARATOR).collect();
        titles.push(segments[1..].join(SEPARATOR));
        titles.push(segments[1].to_owned());
        titles.push(segments[segments.len() - 1].to_owned());
    }
    let mut unique: Vec<String> = Vec::with_capacity(titles.len());
    for title in titles {
        if !unique.contains(&title) {
            unique.push(title);
        }
    }
    unique
}

/// The two campaign-length runs, checked to be the region-prefixed long names
/// and the bare short names in that order.
fn the_two_campaign_runs(rows: &[StringRow]) -> Vec<(u32, u32)> {
    let present = present_ids(rows);
    let runs = maximal_runs(&present);
    let (campaign, _) = directory_campaign();
    let campaign_runs = runs_of_campaign_length(&runs, campaign);
    assert_eq!(
        campaign_runs.len(),
        2,
        "the installation does not offer exactly two campaign-length runs: {campaign_runs:?}"
    );
    for id in campaign_runs[0].0..=campaign_runs[0].1 {
        let text = row_display(rows, id);
        let tail = text
            .find(SEPARATOR)
            .filter(|at| *at > 0)
            .map(|at| &text[at + SEPARATOR.len()..]);
        assert!(
            tail.is_some_and(|tail| !tail.is_empty()),
            "row {id} ({:?}) is not a region-prefixed long name",
            text.chars().take(40).collect::<String>()
        );
    }
    for id in campaign_runs[1].0..=campaign_runs[1].1 {
        let text = row_display(rows, id);
        assert!(
            !text.contains(SEPARATOR),
            "row {id} ({:?}) is not a bare short name",
            text.chars().take(40).collect::<String>()
        );
    }
    campaign_runs
}

// -------------------------------------------------------------- the tests ---

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f50_e4_the_row_runs_are_exactly_the_ones_an_independent_reading_finds() {
    let context = context();
    let rows = context.string_rows();

    // --- the campaign the layout declares, measured here ------------------
    let (campaign, chapter_sizes) = directory_campaign();
    assert_eq!(
        campaign,
        context.campaign().len(),
        "the campaign length this stage compares row runs against is not the one the directory \
         layout declares"
    );
    assert_eq!(
        chapter_sizes,
        context.chapter_sizes(),
        "the per-chapter mission counts this stage measured differ from the ones the binding \
         production code measures"
    );
    assert_eq!(
        chapter_sizes,
        vec![5, 5, 5, 5, 4],
        "the layout's chapter sizes are not the measured ones for this installation"
    );

    // --- the row geometry, re-derived from the table ----------------------
    let present = present_ids(rows);
    let runs = maximal_runs(&present);
    assert_eq!(
        present.len(),
        1207,
        "the number of rows carrying display text is not the one measured for this installation \
         ({STRING_ASSET} decodes 1616 rows, 369 of which hold no text)"
    );
    assert_eq!(
        runs.len(),
        76,
        "the number of maximal runs is not the one measured for this installation"
    );
    // Maximality and disjointness are properties of *this* derivation, checked
    // against the table and not against any other reader of runs.
    for &(first, last) in &runs {
        assert!(
            first == 0 || !carries(rows, first - 1),
            "run {first}..={last} could be extended backwards: row {} carries text",
            first - 1
        );
        assert!(
            !carries(rows, last + 1),
            "run {first}..={last} could be extended forwards: row {} carries text",
            last + 1
        );
    }
    for pair in runs.windows(2) {
        assert!(
            pair[0].1 + 1 < pair[1].0,
            "runs {:?} and {:?} are not ascending and disjoint",
            pair[0],
            pair[1]
        );
    }
    assert_eq!(
        runs.iter().map(|run| width(*run)).max(),
        Some(90),
        "the longest run is not the measured one (40081..=40170)"
    );

    // --- the runs that are as long as the campaign ------------------------
    let campaign_runs = runs_of_campaign_length(&runs, campaign);
    assert_eq!(
        campaign_runs,
        vec![(3450, 3473), (3480, 3503)],
        "the campaign-length runs are not the two measured ones: the region-prefixed long names \
         and the bare short names"
    );
    // This installation offers runs of 23 and of 25 rows as well, so a rule
    // that accepted "about the campaign length" would report four runs where
    // the exact rule reports two.
    let mut widths: BTreeMap<usize, usize> = BTreeMap::new();
    for run in &runs {
        *widths.entry(width(*run)).or_default() += 1;
    }
    assert_eq!(
        widths.get(&(campaign - 1)).copied(),
        Some(1),
        "the installation no longer offers a run one row shorter than the campaign: {widths:?}"
    );
    assert_eq!(
        widths.get(&(campaign + 1)).copied(),
        Some(2),
        "the installation no longer offers runs one row longer than the campaign: {widths:?}"
    );

    // --- production is held to that set, in both directions ---------------
    let reported: Vec<(u32, u32)> = context
        .campaign_title_blocks()
        .into_iter()
        .map(|block| (block.first_id(), block.last_id()))
        .collect();
    for run in &campaign_runs {
        assert!(
            reported.contains(run),
            "the independent reading found the campaign-length run {run:?} and \
             campaign_title_blocks did not report it: {reported:?}"
        );
    }
    for block in &reported {
        assert!(
            campaign_runs.contains(block),
            "campaign_title_blocks reported {block:?}, which is not a campaign-length run of the \
             table: {campaign_runs:?}"
        );
        assert_eq!(
            width(*block),
            campaign,
            "campaign_title_blocks reported a {}-row run for a {campaign}-mission campaign",
            width(*block)
        );
    }
    for run in &runs {
        if width(*run) != campaign {
            assert!(
                !reported.contains(run),
                "campaign_title_blocks reported {run:?}, which is {} rows and not as long as the \
                 campaign",
                width(*run)
            );
        }
    }
    assert_eq!(
        reported.len(),
        campaign_runs.len(),
        "campaign_title_blocks reported {} runs, the independent reading found {}",
        reported.len(),
        campaign_runs.len()
    );
    for pair in reported.windows(2) {
        assert!(
            pair[0].1 < pair[1].0,
            "campaign_title_blocks reported runs that are not ascending and disjoint: {reported:?}"
        );
    }
    // And the near misses of the *rule*: a truncated or extended version of a
    // campaign-length run is not one, even though every row of it carries text.
    for (first, last) in &campaign_runs {
        for candidate in [
            (*first, last - 1),
            (first + 1, *last),
            (*first, last + 1),
            (first - 1, *last),
        ] {
            assert!(
                !reported.contains(&candidate),
                "campaign_title_blocks reported {candidate:?}, which is not a run of the table"
            );
        }
    }

    // --- the 48 rows are text rows in the image, by their own bytes -------
    //
    // The geometry above is measured over decoded rows. Re-deriving each
    // campaign row's own byte range from the decoded units of its block, and
    // decoding those bytes back, is what ties the geometry to the strings the
    // installation actually carries instead of to this crate's reading of them.
    let image = fs::read(game_dir().join(STRING_ASSET))
        .unwrap_or_else(|error| panic!("cannot read {STRING_ASSET}: {error}"));
    let ids = campaign_rows(&campaign_runs);
    assert_eq!(ids.len(), 2 * campaign);
    for id in ids {
        let row = rows
            .iter()
            .find(|row| row.id == id)
            .unwrap_or_else(|| panic!("the table has no row {id}"));
        let mut offset = row.span.offset();
        let mut located = false;
        for candidate in rows {
            if candidate.span != row.span {
                continue;
            }
            let length =
                2 + 2 * u64::try_from(candidate.code_units.len()).expect("length fits u64");
            if candidate.id == row.id {
                let start = usize::try_from(offset).expect("offset fits usize");
                let end = start + usize::try_from(length).expect("length fits usize");
                assert!(
                    end <= image.len(),
                    "row {id}'s re-derived range runs past the end of {STRING_ASSET}"
                );
                let bytes = &image[start..end];
                let count = usize::from(u16::from_le_bytes([bytes[0], bytes[1]]));
                assert_eq!(
                    count,
                    candidate.code_units.len(),
                    "row {id}'s re-derived range does not start at its own code-unit count"
                );
                let units: Vec<u16> = bytes[2..]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| u16::from_le_bytes(*pair))
                    .collect();
                assert_eq!(
                    units, candidate.code_units,
                    "row {id}'s re-derived range is not the row's own code units"
                );
                located = true;
                break;
            }
            offset += length;
        }
        assert!(located, "row {id} is not in the block it reports");
        assert!(
            !display(row).is_empty(),
            "row {id} is in a campaign-length run but carries no comparable text"
        );
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f50_e4_the_confirmed_rows_are_exactly_the_exact_byte_matches() {
    let context = context();
    let rows = context.string_rows();
    let campaign_runs = the_two_campaign_runs(rows);
    let index = carried_titles(rows);
    let titles = near_miss_titles(rows, &campaign_runs);
    assert!(
        titles.len() > 200,
        "the near-miss set is smaller than expected: {} titles",
        titles.len()
    );

    // --- the per-row rule, held to this file's own reading ----------------
    //
    // `title_form` is compared row by row rather than through `confirm_title`
    // alone, because a confirmation reports *one* row in *one* form: a rule that
    // picked the wrong row, or called a long name verbatim, could hide behind an
    // agreeing confirmation. Every row of the table is compared, not only the
    // campaign ones — a rule that fired on the rest of the localized strings
    // would be a different rule.
    let mut compared = 0usize;
    for row in rows {
        let text = display(row);
        if text.is_empty() {
            continue;
        }
        for title in &titles {
            assert_eq!(
                title_form(text, title).map(from_production),
                carried_by(text, title),
                "row {} ({:?}) and title {title:?}: production and the independent reading \
                 disagree about how the row carries the title",
                row.id,
                text.chars().take(40).collect::<String>()
            );
            compared += 1;
        }
    }
    assert_eq!(
        compared,
        present_ids(rows).len() * titles.len(),
        "not every row was compared against every candidate title"
    );

    // --- the confirmation, held to the inverted index ---------------------
    let mut classes: BTreeMap<&str, usize> = BTreeMap::new();
    for title in &titles {
        let expected = predict(&index, title);
        let actual = context.confirm_title(title);
        match &expected {
            Expected::Confirmed(id, form) => {
                let (row, production_form) = actual.confirmed().unwrap_or_else(|| {
                    panic!(
                        "{title:?} is carried byte for byte by row {id} in the expected form, and \
                         confirm_title reported {actual:?}"
                    )
                });
                assert_eq!(row, *id, "{title:?} confirms the wrong row");
                assert_eq!(
                    production_form,
                    as_production(*form),
                    "{title:?} is carried by row {id} in the wrong display form"
                );
                assert_eq!(
                    actual.refusal(),
                    None,
                    "{title:?} confirmed and refused at once"
                );
            }
            Expected::Ambiguous { ids, form } => {
                assert_eq!(
                    actual,
                    TitleConfirmation::Ambiguous,
                    "{title:?} is carried by {ids:?} in the same form, so no single row names it, \
                     and confirm_title reported {actual:?}"
                );
                for id in ids {
                    assert_eq!(
                        title_form(row_display(rows, *id), title),
                        Some(as_production(*form)),
                        "row {id} is one of the rows carrying {title:?} but does not carry it in \
                         the form the ambiguity is about"
                    );
                }
            }
            Expected::Uncarried => assert_eq!(
                actual,
                TitleConfirmation::Uncarried,
                "no row carries {title:?} byte for byte, and confirm_title reported {actual:?}"
            ),
        }
        *classes
            .entry(match expected {
                Expected::Confirmed(_, Form::Whole) => "confirmed verbatim",
                Expected::Confirmed(_, Form::Tail) => "confirmed through a long name",
                Expected::Ambiguous { .. } => "ambiguous",
                Expected::Uncarried => "uncarried",
            })
            .or_default() += 1;
    }
    // Every arm has to be exercised, or the comparison above is a formality, and
    // the counts are pinned so a candidate set that quietly stopped covering an
    // arm cannot pass as one that still does.
    for (class, expected) in [
        ("confirmed verbatim", 27),
        ("confirmed through a long name", 19),
        ("ambiguous", 2),
        ("uncarried", 171),
    ] {
        let seen = classes.get(class).copied().unwrap_or(0);
        assert_eq!(
            seen, expected,
            "the near-miss set no longer splits the way it was measured: {classes:?}"
        );
    }

    // --- the arms this installation does produce, named -------------------
    //
    // Seventeen of the twenty-four declared titles are carried and seven are
    // not; of the carried ones exactly one is confirmed only through its long
    // name, because the bare short name of that mission omits a word the
    // declared title carries.
    let mut carried: Vec<(String, String, u32, Form)> = Vec::new();
    let mut uncarried: Vec<String> = Vec::new();
    for (label, title) in declared_titles() {
        match predict(&index, &title) {
            Expected::Confirmed(id, form) => carried.push((label, title, id, form)),
            _ => uncarried.push(label),
        }
    }
    assert_eq!(
        carried.len(),
        17,
        "the declared titles this stage confirmed: {carried:?}"
    );
    assert_eq!(
        uncarried,
        vec!["M09", "M11", "M14", "M15", "M20", "M22", "M23"],
        "the declared titles the installation does not carry byte for byte changed"
    );
    assert_eq!(
        carried
            .iter()
            .filter(|(_, _, _, form)| *form == Form::Tail)
            .count(),
        1,
        "more than one declared title is confirmed only through a long name: {carried:?}"
    );
    for (label, title, id, form) in &carried {
        let block = campaign_runs
            .iter()
            .find(|(first, last)| *first <= *id && *id <= *last)
            .unwrap_or_else(|| {
                panic!(
                    "{label}'s title {title:?} is confirmed at row {id}, outside every \
                     campaign-length run"
                )
            });
        assert_eq!(
            *form == Form::Tail,
            *block == campaign_runs[0],
            "{label}'s title {title:?} confirms at row {id} in run {block:?}, which is the {} run",
            if *form == Form::Tail {
                "region-prefixed long name"
            } else {
                "bare short name"
            }
        );
    }

    // Both display forms carry six of the confirmed titles, and the verbatim
    // row wins every one of them: the rule's order, on real data.
    let carried_both_ways: Vec<&(String, String, u32, Form)> = carried
        .iter()
        .filter(|(_, title, _, _)| {
            index.get(title).is_some_and(|carriers| {
                carriers.iter().any(|(_, form)| *form == Form::Whole)
                    && carriers.iter().any(|(_, form)| *form == Form::Tail)
            })
        })
        .collect();
    assert_eq!(
        carried_both_ways.len(),
        6,
        "the number of confirmed titles a long name also carries changed: {carried_both_ways:?}"
    );
    for (label, title, id, _) in carried_both_ways {
        let long_row = index
            .get(title)
            .expect("the title is in the index")
            .iter()
            .find(|(_, form)| *form == Form::Tail)
            .expect("it is carried by a long name too")
            .0;
        assert!(
            campaign_runs[1].0 <= *id && *id <= campaign_runs[1].1,
            "{label}'s title {title:?} is carried by the long name at row {long_row} and by a row \
             at {id} outside the bare-short-name run, so the verbatim row did not win"
        );
    }

    // A region prefix is title-shaped, and the table carries some of them more
    // than once: two of the five distinct prefixes of the long-name run are
    // ambiguous, so they select no campaign position even though they are real
    // strings. This is the arm M18 measured for one prefix; here every distinct
    // prefix of the run is held to the independent reading.
    let prefixes: BTreeSet<String> = (campaign_runs[0].0..=campaign_runs[0].1)
        .filter_map(|id| {
            let text = row_display(rows, id);
            text.find(SEPARATOR)
                .filter(|at| *at > 0)
                .map(|at| text[..at].to_owned())
        })
        .collect();
    assert_eq!(
        prefixes.len(),
        5,
        "the long-name run does not offer five distinct region prefixes: {prefixes:?}"
    );
    let mut prefix_arms: BTreeMap<&str, usize> = BTreeMap::new();
    for prefix in &prefixes {
        let expected = predict(&index, prefix);
        let actual = context.confirm_title(prefix);
        match (&expected, actual) {
            (
                Expected::Confirmed(row, Form::Whole),
                TitleConfirmation::Confirmed { row_id: found, .. },
            ) => {
                assert_eq!(
                    found, *row,
                    "the region prefix {prefix:?} confirmed the wrong row"
                );
                assert!(
                    !campaign_runs
                        .iter()
                        .any(|(first, last)| *first <= *row && *row <= *last),
                    "the region prefix {prefix:?} confirmed inside a campaign-length run"
                );
            }
            (Expected::Ambiguous { .. }, TitleConfirmation::Ambiguous) => {}
            _ => panic!(
                "the region prefix {prefix:?} is {expected:?} for the independent reading and \
                 {actual:?} for production"
            ),
        }
        *prefix_arms
            .entry(match expected {
                Expected::Confirmed(_, _) => "confirmed",
                _ => "ambiguous",
            })
            .or_default() += 1;
    }
    assert_eq!(
        prefix_arms.get("confirmed").copied().unwrap_or(0),
        3,
        "the region prefixes the table carries exactly once changed: {prefix_arms:?}"
    );
    assert_eq!(
        prefix_arms.get("ambiguous").copied().unwrap_or(0),
        2,
        "the region prefixes the table carries more than once changed: {prefix_arms:?}"
    );
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f50_e4_a_fuzzy_matcher_would_confirm_a_near_miss_this_table_refuses() {
    let context = context();
    let rows = context.string_rows();
    let campaign_runs = the_two_campaign_runs(rows);
    let titles = near_miss_titles(rows, &campaign_runs);
    let index = carried_titles(rows);

    // Three weakened matchers, each written over the *table*, so that what each
    // would confirm is measured rather than assumed: a display or tail that
    // merely starts with the title, a display that merely contains it, and a
    // comparison that ignores case.
    fn prefix(text: &str, title: &str) -> bool {
        text == title
            || text
                .find(SEPARATOR)
                .filter(|at| *at > 0)
                .is_some_and(|at| text[at + SEPARATOR.len()..].starts_with(title))
    }

    fn substring(text: &str, title: &str) -> bool {
        !title.is_empty() && text.contains(title)
    }

    fn case_insensitive(text: &str, title: &str) -> bool {
        let text = text.to_lowercase();
        let title = title.to_lowercase();
        text == title
            || text
                .find(SEPARATOR)
                .filter(|at| *at > 0)
                .is_some_and(|at| text[at + SEPARATOR.len()..] == title)
    }

    let would_confirm = |title: &str, weaker: fn(&str, &str) -> bool| -> Vec<u32> {
        rows.iter()
            .filter(|row| {
                let text = display(row);
                !text.is_empty() && weaker(text, title)
            })
            .map(|row| row.id)
            .collect()
    };

    let mut counted: BTreeMap<&str, usize> = BTreeMap::new();
    for (name, weaker) in [
        (
            "a tail that merely starts with the title",
            prefix as fn(&str, &str) -> bool,
        ),
        ("a display that merely contains the title", substring),
        ("a comparison that ignores case", case_insensitive),
    ] {
        let wrongly: Vec<&String> = titles
            .iter()
            .filter(|title| {
                !would_confirm(title, weaker).is_empty()
                    && predict(&index, title) == Expected::Uncarried
            })
            .collect();
        assert!(
            wrongly.len() >= 10,
            "the {name} matcher would confirm only {} near misses out of {} titles, which is too \
             few to catch a fuzzy rule: {wrongly:?}",
            wrongly.len(),
            titles.len()
        );

        for title in &wrongly {
            let actual = context.confirm_title(title);
            assert_eq!(
                actual,
                TitleConfirmation::Uncarried,
                "{title:?} is refused by an exact comparison while the {name} matcher would have \
                 accepted row(s) {:?}, but confirm_title reported {actual:?}",
                would_confirm(title, weaker)
            );
        }
        *counted.entry(name).or_default() += wrongly.len();
    }
    // The measured size of the set each weakened matcher would take, pinned: a
    // table that stopped offering near misses must fail here rather than leave
    // the arm vacuously satisfied.
    assert_eq!(
        counted,
        BTreeMap::from([
            ("a comparison that ignores case", 48),
            ("a display that merely contains the title", 72),
            ("a tail that merely starts with the title", 27),
        ]),
        "the weakened matchers' hauls over this table changed: {counted:?}"
    );

    // The seven declared titles the installation does not carry are near misses
    // of real rows, not empty space: for each, some row of the table is within a
    // few edits of it, and a fuzzy matcher would have picked one of them.
    let mut distances = Vec::new();
    for (label, title) in declared_titles() {
        if predict(&index, &title) != Expected::Uncarried {
            continue;
        }
        let (distance, id) = rows
            .iter()
            .filter(|row| !display(row).is_empty())
            .map(|row| {
                (
                    edit_distance(&title.to_lowercase(), &display(row).to_lowercase()),
                    row.id,
                )
            })
            .min()
            .expect("the table has rows");
        assert!(
            distance >= 1,
            "{label}'s title {title:?} is reported as uncarried while row {id} is identical to it"
        );
        assert!(
            distance <= 5,
            "{label}'s title {title:?} is uncarried, but the nearest row ({id}) is {distance} edits \
             away, which is too far for the table's refusal to mean anything"
        );
        distances.push(format!("{label}: row {id} at {distance} edits"));
    }
    assert_eq!(
        distances.len(),
        7,
        "the number of declared titles the table refuses changed: {distances:?}"
    );
}
