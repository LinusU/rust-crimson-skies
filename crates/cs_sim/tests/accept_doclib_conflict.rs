//! Guard for the crate module-doc order convention (task #474, DOCLIB-APPSIM).
//!
//! The rule is written down in `docs/architecture/crate-module-docs.md`: a
//! crate's `src/lib.rs` module-doc paragraphs appear in ascending feature-sheet
//! order, and the `pub mod` list stays alphabetical. These tests read this
//! crate's real `src/lib.rs`, embedded at compile time, so a stage branch that
//! adds its paragraph at the old shared append anchor fails here with a pointer
//! to the rule instead of reopening the merge hotspot. Task #473 shipped the
//! same guard for `cs_content` and `cs_formats`; this crate's stage paragraphs
//! were brought into the convention here.

const LIB_RS: &str = include_str!("../src/lib.rs");

/// The crate-level `//!` paragraphs, in file order, each as its content lines.
fn doc_paragraphs(source: &str) -> Vec<Vec<String>> {
    let mut block: Vec<&str> = Vec::new();
    for line in source.lines() {
        if line.starts_with("//!") || line.trim().is_empty() {
            block.push(line);
        } else {
            break;
        }
    }

    let mut paragraphs: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    for line in block {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed == "//!" {
            if !current.is_empty() {
                paragraphs.push(std::mem::take(&mut current));
            }
        } else {
            current.push(line.to_string());
        }
    }
    if !current.is_empty() {
        paragraphs.push(current);
    }
    paragraphs
}

/// The `Fnn` of the paragraph's first `specs/Fnn-` reference, if it has one.
fn first_sheet(paragraph: &[String]) -> Option<u32> {
    let text = paragraph.join("\n");
    let start = text.find("specs/F")? + "specs/F".len();
    let digits: String = text[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

#[test]
fn accept_doclib_conflict_cs_sim_doc_paragraphs_follow_feature_sheet_order() {
    let paragraphs = doc_paragraphs(LIB_RS);
    assert!(
        paragraphs.len() > 1,
        "the crate-doc block must contain paragraphs"
    );

    let mut previous: Option<(u32, usize)> = None;
    let mut first_sheet_paragraph: Option<usize> = None;
    let mut last_sheet_paragraph: Option<usize> = None;

    for (index, paragraph) in paragraphs.iter().enumerate() {
        match first_sheet(paragraph) {
            Some(sheet) => {
                if let Some((previous_sheet, previous_index)) = previous {
                    assert!(
                        sheet >= previous_sheet,
                        "accept_doclib_conflict: crate-doc paragraph {index} documents F{sheet:02} \
                         after paragraph {previous_index} documented F{previous_sheet:02}; \
                         module-doc paragraphs must be inserted in ascending feature-sheet order \
                         (docs/architecture/crate-module-docs.md)"
                    );
                }
                previous = Some((sheet, index));
                first_sheet_paragraph.get_or_insert(index);
                last_sheet_paragraph = Some(index);
            }
            None => {
                if let Some(first) = first_sheet_paragraph {
                    assert!(
                        index < first
                            || index > last_sheet_paragraph.expect("last sheet paragraph"),
                        "accept_doclib_conflict: crate-doc paragraph {index} has no `specs/Fnn-` \
                         reference between two sheet paragraphs; the crate introduction and closing \
                         notes stay before the first and after the last sheet paragraph \
                         (docs/architecture/crate-module-docs.md)"
                    );
                }
            }
        }
    }

    assert!(
        first_sheet_paragraph.is_some(),
        "expected at least one paragraph naming a feature sheet"
    );
}

#[test]
fn accept_doclib_conflict_cs_sim_module_list_is_alphabetical() {
    let modules: Vec<&str> = LIB_RS
        .lines()
        .filter_map(|line| line.strip_prefix("pub mod "))
        .filter_map(|rest| rest.strip_suffix(';'))
        .collect();
    assert!(!modules.is_empty(), "expected a `pub mod` list");

    let mut sorted = modules.clone();
    sorted.sort_unstable();
    assert_eq!(
        modules, sorted,
        "accept_doclib_conflict: the `pub mod` list must stay alphabetical \
         (docs/architecture/crate-module-docs.md)"
    );
}
