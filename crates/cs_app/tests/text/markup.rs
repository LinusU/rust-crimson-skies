//! The declared control-markup grammar: what it admits, and what it refuses
//! (F51-A).
//!
//! Non-negotiable behavior 2: a resource string is retained, and its control
//! markup, line breaks and substitutions are interpreted, **only after grammar
//! validation**. Anything the grammar does not admit stays literal text and
//! produces a diagnostic, so no resource string is ever executable markup.

use cs_content::localization::{
    MAX_MARKUP_TOKEN_LEN, MarkupDelimiterError, MarkupGrammar, MarkupGrammarError, MarkupToken,
    SubstitutionTable, SubstitutionValueError, UNRESOLVED_SUBSTITUTION, parse_markup,
};

use crate::common::{document, grammar};

/// A string the grammar admits becomes tokens, and its hard line breaks are
/// kept.
#[test]
fn accept_f51_a_admitted_markup_becomes_tokens_with_its_line_breaks() {
    let document = document(
        "[color=red]Attention[/color] flight [bold]crew[/bold].\nSecond line for {pilot}.",
    );
    assert!(
        document.is_clean(),
        "issues: {:?}",
        document
            .issues()
            .iter()
            .map(|i| i.detail.clone())
            .collect::<Vec<_>>()
    );
    let tokens = document.tokens();
    assert!(
        matches!(&tokens[0], MarkupToken::Control { name, argument, closing }
        if name == "color" && argument.as_deref() == Some("red") && !*closing)
    );
    assert!(matches!(
        tokens
            .iter()
            .filter(|t| matches!(t, MarkupToken::LineBreak))
            .count(),
        1
    ));
    assert!(
        tokens
            .iter()
            .filter(|t| matches!(t, MarkupToken::Control { .. }))
            .count()
            == 4,
        "two opening and two closing controls"
    );
    assert!(
        tokens
            .iter()
            .any(|token| matches!(token, MarkupToken::Substitution { id } if id == "pilot"))
    );

    let mut substitutions = SubstitutionTable::new();
    substitutions
        .insert("pilot", "Mara")
        .expect("a plain value is admitted");
    let (paragraphs, unresolved) = document.paragraphs(&substitutions);
    assert_eq!(paragraphs.len(), 2, "the hard line break split the string");
    assert_eq!(paragraphs[1], "Second line for Mara.");
    assert!(unresolved.is_empty());
}

/// A supplied value may not smuggle in the unresolved-substitution marker, and
/// its id must be writable as `{id}`. Otherwise a marker in the rendered text
/// would stop meaning "this substitution was unresolved", and a layout counting
/// markers to place its diagnostics would blame the wrong id.
#[test]
fn accept_f51_a_a_substitution_value_may_not_impersonate_the_marker() {
    let mut substitutions = SubstitutionTable::new();
    assert_eq!(
        substitutions
            .insert("pilot", "Ma\u{fffd}ra")
            .expect_err("a value carrying the marker is refused"),
        SubstitutionValueError::MarkerInValue {
            id: "pilot".to_owned()
        }
    );
    assert_eq!(
        substitutions
            .insert("", "Mara")
            .expect_err("an empty id is refused"),
        SubstitutionValueError::BadId { id: String::new() }
    );
    assert_eq!(
        substitutions
            .insert("first name", "Mara")
            .expect_err("an id outside the token grammar is refused"),
        SubstitutionValueError::BadId {
            id: "first name".to_owned()
        }
    );
    assert!(substitutions.is_empty(), "a refused value is never stored");

    // An empty value is admitted: an empty *value* is the caller's data, and it
    // is not the same defect as an unresolved substitution, which is a missing
    // entry rather than an empty one.
    substitutions
        .insert("callsign", "")
        .expect("an empty value is the caller's choice");
    assert_eq!(substitutions.get("callsign"), Some(""));
    assert!(substitutions.contains("callsign"));

    // The rendered text therefore contains a marker only for a genuinely
    // unresolved id.
    let document = document("Callsign {callsign} ready, pilot {pilot}.");
    let (paragraphs, unresolved) = document.paragraphs(&substitutions);
    assert_eq!(paragraphs, vec!["Callsign  ready, pilot \u{fffd}."]);
    assert_eq!(unresolved, vec!["pilot".to_owned()]);
}

/// The positional form keeps *which paragraph* an unresolved id is in, and with
/// repeats, so a layout can name the line a marker landed on instead of
/// guessing from a deduplicated global list.
#[test]
fn accept_f51_a_paragraph_substitutions_keep_the_positional_markers() {
    let document = document("Ready {pilot}.\nCorridor {runway} via {pilot}.\nClear.");
    let (paragraphs, per_paragraph) = document.paragraph_substitutions(&SubstitutionTable::new());
    assert_eq!(
        paragraphs.len(),
        3,
        "two hard line breaks make three paragraphs"
    );
    assert_eq!(per_paragraph.len(), 3, "one entry per paragraph");
    assert_eq!(per_paragraph[0], vec!["pilot".to_owned()]);
    assert_eq!(
        per_paragraph[1],
        vec!["runway".to_owned(), "pilot".to_owned()],
        "the order the markers render in, with the repeat kept"
    );
    assert!(
        per_paragraph[2].is_empty(),
        "the last paragraph has no marker"
    );
    for paragraph in &paragraphs[0..2] {
        assert!(paragraph.contains(UNRESOLVED_SUBSTITUTION));
    }

    // The deduplicated view is the same ids in first-appearance order.
    let (paragraphs, unresolved) = document.paragraphs(&SubstitutionTable::new());
    assert_eq!(paragraphs.len(), 3);
    assert_eq!(unresolved, vec!["pilot".to_owned(), "runway".to_owned()]);

    // A supplied value removes exactly that id from the positional list.
    let mut substitutions = SubstitutionTable::new();
    substitutions
        .insert("pilot", "Mara")
        .expect("a plain value is admitted");
    let (paragraphs, per_paragraph) = document.paragraph_substitutions(&substitutions);
    assert_eq!(paragraphs[0], "Ready Mara.");
    assert!(per_paragraph[0].is_empty());
    assert_eq!(
        per_paragraph[1],
        vec!["runway".to_owned()],
        "supplying the pilot removes only that id, and the repeat goes with it"
    );
    assert_eq!(
        paragraphs[1],
        format!("Corridor {UNRESOLVED_SUBSTITUTION} via Mara.")
    );
}

/// Every markup problem the grammar can find is reported, and the refused
/// control stays verbatim in the text instead of being dropped or executed.
#[test]
fn accept_f51_a_refused_markup_stays_literal_text_and_is_reported() {
    // (source, expected issue code, must the refused markup stay literal?)
    let cases: &[(&str, &str, bool)] = &[
        ("[blink]x[/blink]", "unknown_tag", true),
        ("[bold=1]x[/bold]", "unexpected_argument", true),
        ("[/color]", "unbalanced_control", true),
        ("[color=red", "unterminated_control", true),
        ("[]x[]", "empty_tag", true),
        ("{}x{}", "empty_substitution", true),
        ("{unterminated", "unterminated_substitution", true),
        // An id that is present but outside the token grammar is named as such,
        // not mislabelled as an empty substitution.
        ("{pilot name}", "bad_substitution_id", true),
        // An unclosed control was admitted; only the missing close is reported,
        // so the control itself is not literal text.
        ("[color=red]x", "unclosed_control", false),
    ];
    for (source, expected_code, literal) in cases {
        let document = document(source);
        assert!(!document.is_clean(), "{source:?} must produce a diagnostic");
        let codes: Vec<&str> = document.issues().iter().map(|i| i.code()).collect();
        assert!(
            codes.contains(expected_code),
            "{source:?}: expected {expected_code}, got {codes:?}"
        );
        let rendered: String = document
            .tokens()
            .iter()
            .map(|token| match token {
                MarkupToken::Text(text) => text.as_str(),
                MarkupToken::LineBreak => "\n",
                MarkupToken::Control { .. } => "",
                MarkupToken::Substitution { .. } => "",
            })
            .collect();
        if *literal {
            // The refused markup is still in the output as literal text, so the
            // displayed string is the resource string and nothing vanished.
            let refused = &source[source.find(['[', '{']).expect("a delimiter")..];
            assert!(
                rendered.contains(refused),
                "{source:?}: the refused control must stay literal, rendered {rendered:?}"
            );
        } else {
            assert!(
                document.tokens().iter().any(
                    |token| matches!(token, MarkupToken::Control { name, .. } if name == "color")
                ),
                "{source:?}: the admitted control must still be a token"
            );
        }
    }
}

/// The problem carries the byte offset in the original string, so a diagnostic
/// points at the exact place in the resource that has to be fixed.
#[test]
fn accept_f51_a_a_markup_diagnostic_points_at_the_original_string() {
    let source = "ok [blink]x[/blink]";
    let document = document(source);
    let issue = document
        .issues()
        .iter()
        .find(|issue| issue.code() == "unknown_tag")
        .expect("the unknown tag is reported");
    assert_eq!(
        issue.offset(),
        Some(source.find('[').expect("an offset exists"))
    );
    assert!(issue.to_string().contains("blink"));
}

/// A caller that declares a different delimiter spelling is honoured end to
/// end: the grammar is the only source of the syntax, and the default spelling
/// is just this project's declared choice.
#[test]
fn accept_f51_a_the_caller_declares_the_delimiter_spelling() {
    let declared = grammar();
    assert_eq!(declared.delimiters().control_open, '[');

    let guillemets = MarkupGrammar::new(
        cs_content::localization::MarkupDelimiters {
            control_open: '\u{ab}',
            control_close: '\u{bb}',
            substitution_open: '\u{2039}',
            substitution_close: '\u{203a}',
        },
        [cs_content::localization::ControlTag::simple("bold").expect("the tag name is valid")],
    )
    .expect("the guillemet grammar is valid");
    let document = parse_markup(
        "\u{ab}bold\u{bb}crew\u{ab}/bold\u{bb}\u{2039}pilot\u{203a}",
        &guillemets,
    );
    assert!(document.is_clean(), "{:?}", document.issues());
    // The declared `[bold]` spelling is now plain text under this grammar: no
    // delimiter of the guillemet grammar appears, so nothing is interpreted and
    // nothing is reported either.
    let brackets = parse_markup("[bold]crew[/bold]", &guillemets);
    assert!(brackets.is_clean(), "{:?}", brackets.issues());
    assert!(
        brackets
            .tokens()
            .iter()
            .all(|token| matches!(token, MarkupToken::Text(_)))
    );
    let rendered: String = brackets
        .tokens()
        .iter()
        .map(|token| match token {
            MarkupToken::Text(text) => text.as_str(),
            MarkupToken::LineBreak => "\n",
            MarkupToken::Control { .. } | MarkupToken::Substitution { .. } => "",
        })
        .collect();
    assert_eq!(rendered, "[bold]crew[/bold]");
}

/// A grammar is validated: delimiters are four distinct non-alphanumeric
/// characters, and a tag name is a bounded token declared at most once.
#[test]
fn accept_f51_a_a_malformed_grammar_is_refused() {
    use cs_content::localization::{ControlTag, MarkupDelimiters, MarkupGrammarErrorOrDelimiters};

    assert_eq!(
        MarkupGrammar::new(
            MarkupDelimiters {
                control_open: 'a',
                control_close: ']',
                substitution_open: '{',
                substitution_close: '}',
            },
            [ControlTag::simple("bold").expect("valid")],
        )
        .expect_err("an alphanumeric delimiter is refused"),
        MarkupGrammarErrorOrDelimiters::Delimiters(MarkupDelimiterError::Alphanumeric { ch: 'a' })
    );
    assert_eq!(
        MarkupGrammar::new(
            MarkupDelimiters {
                control_open: '[',
                control_close: '[',
                substitution_open: '{',
                substitution_close: '}',
            },
            [ControlTag::simple("bold").expect("valid")],
        )
        .expect_err("a shared delimiter is refused"),
        MarkupGrammarErrorOrDelimiters::Delimiters(MarkupDelimiterError::Duplicate { ch: '[' })
    );
    assert_eq!(
        MarkupGrammar::new(MarkupDelimiters::DECLARED, [])
            .expect_err("a grammar with no tags is refused"),
        MarkupGrammarErrorOrDelimiters::Grammar(MarkupGrammarError::NoTags)
    );
    assert_eq!(
        MarkupGrammar::new(
            MarkupDelimiters::DECLARED,
            [
                ControlTag::simple("bold").expect("valid"),
                ControlTag::simple("bold").expect("valid"),
            ],
        )
        .expect_err("a duplicated tag is refused"),
        MarkupGrammarErrorOrDelimiters::Grammar(MarkupGrammarError::DuplicateTag {
            name: "bold".to_owned()
        })
    );
    assert_eq!(
        ControlTag::simple(&"x".repeat(MAX_MARKUP_TOKEN_LEN + 1))
            .expect_err("a long tag is refused"),
        MarkupGrammarError::BadTagName {
            name: "x".repeat(MAX_MARKUP_TOKEN_LEN + 1)
        }
    );
}

/// The token stream says which control opens and which closes, and the two pair
/// up in order: a renderer pushes the style an opening control names and pops
/// it at the matching close, and it can only do that if the stream distinguishes
/// them. A close that does not match the innermost open tag is refused instead.
#[test]
fn accept_f51_a_a_closing_control_is_distinguishable_from_an_opening_one() {
    let document = document("[color=red]Warning[bold] now[/bold][/color]");
    assert!(document.is_clean(), "{:?}", document.issues());

    let controls: Vec<(&str, Option<&str>, bool)> = document
        .tokens()
        .iter()
        .filter_map(|token| match token {
            MarkupToken::Control {
                name,
                argument,
                closing,
            } => Some((name.as_str(), argument.as_deref(), *closing)),
            _ => None,
        })
        .collect();
    assert_eq!(
        controls,
        vec![
            ("color", Some("red"), false),
            ("bold", None, false),
            ("bold", None, true),
            ("color", None, true),
        ],
        "two opening and two closing controls, in order"
    );

    // The helper a renderer uses: push on an open, pop on a close, and the
    // stack is empty again at the end of the string.
    let mut stack: Vec<&str> = Vec::new();
    for token in document.tokens() {
        if token.is_opening() {
            stack.push(token.control_name().expect("a control names its tag"));
        } else if token.is_closing() {
            assert_eq!(
                stack.pop(),
                token.control_name(),
                "a closing control must pop the tag it names"
            );
        }
    }
    assert!(stack.is_empty(), "every opening control was closed");
    assert!(
        document
            .tokens()
            .iter()
            .all(|token| !token.is_closing() || token.control_name().is_some())
    );

    // Text and substitution tokens are neither an opening nor a closing control.
    let plain = crate::common::document("[bold]crew[/bold]");
    for token in plain.tokens() {
        match token {
            MarkupToken::Text(_) | MarkupToken::Substitution { .. } => {
                assert!(!token.is_opening() && !token.is_closing());
                assert!(token.control_name().is_none());
            }
            MarkupToken::LineBreak => assert!(!token.is_opening() && !token.is_closing()),
            MarkupToken::Control { .. } => {}
        }
    }
}

/// A substitution id that is present but outside the token grammar is reported
/// as a *bad id*, naming what was written — a diagnostic that claims "no id"
/// when the author wrote one sends the fix in the wrong direction.
#[test]
fn accept_f51_a_a_malformed_substitution_id_is_named_not_called_empty() {
    let source = "Confirm with {first name}.";
    let document = document(source);
    let issue = document
        .issues()
        .iter()
        .find(|issue| issue.code() == "bad_substitution_id")
        .expect("the bad id is reported");
    assert_eq!(issue.offset(), Some(source.find('{').expect("an offset")));
    assert!(issue.to_string().contains("first name"), "{issue}");
    assert!(
        !document
            .issues()
            .iter()
            .any(|issue| issue.code() == "empty_substitution"),
        "a written id is not an empty substitution: {:?}",
        document.issues()
    );
    // It stays literal text, and it is not admitted as a substitution token.
    let rendered: String = document
        .tokens()
        .iter()
        .map(|token| match token {
            MarkupToken::Text(text) => text.as_str(),
            MarkupToken::LineBreak => "\n",
            MarkupToken::Control { .. } | MarkupToken::Substitution { .. } => "",
        })
        .collect();
    assert_eq!(rendered, source);
    assert!(!document.is_clean());

    // A valid id of the same shape is admitted, so the rule is the grammar and
    // not a blanket refusal of ids.
    let valid = crate::common::document("Confirm with {firstname}.");
    assert!(valid.is_clean(), "{:?}", valid.issues());
    assert!(
        valid
            .tokens()
            .iter()
            .any(|token| matches!(token, MarkupToken::Substitution { id } if id == "firstname"))
    );
}

/// A substitution the screen did not supply renders as a **visible** marker and
/// is named, never as an empty gap that reads as correct output.
#[test]
fn accept_f51_a_an_unresolved_substitution_is_visible_and_named() {
    let document = document("Confirm with {pilot} before {runway}.");
    let (paragraphs, unresolved) = document.paragraphs(&SubstitutionTable::new());
    assert_eq!(paragraphs.len(), 1);
    assert!(paragraphs[0].contains(UNRESOLVED_SUBSTITUTION));
    assert_eq!(unresolved, vec!["pilot".to_owned(), "runway".to_owned()]);

    let mut substitutions = SubstitutionTable::new();
    substitutions
        .insert("pilot", "Mara")
        .expect("a plain value is admitted");
    let (paragraphs, unresolved) = document.paragraphs(&substitutions);
    assert_eq!(paragraphs[0], "Confirm with Mara before \u{fffd}.");
    assert_eq!(unresolved, vec!["runway".to_owned()]);
}
