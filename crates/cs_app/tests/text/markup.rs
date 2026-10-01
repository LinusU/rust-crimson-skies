//! The declared control-markup grammar: what it admits, and what it refuses
//! (F51-A).
//!
//! Non-negotiable behavior 2: a resource string is retained, and its control
//! markup, line breaks and substitutions are interpreted, **only after grammar
//! validation**. Anything the grammar does not admit stays literal text and
//! produces a diagnostic, so no resource string is ever executable markup.

use cs_content::localization::{
    MAX_MARKUP_TOKEN_LEN, MarkupDelimiterError, MarkupGrammar, MarkupGrammarError, MarkupToken,
    SubstitutionTable, UNRESOLVED_SUBSTITUTION, parse_markup,
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
    assert!(matches!(&tokens[0], MarkupToken::Control { name, argument }
        if name == "color" && argument.as_deref() == Some("red")));
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
    substitutions.insert("pilot", "Mara");
    let (paragraphs, unresolved) = document.paragraphs(&substitutions);
    assert_eq!(paragraphs.len(), 2, "the hard line break split the string");
    assert_eq!(paragraphs[1], "Second line for Mara.");
    assert!(unresolved.is_empty());
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
    substitutions.insert("pilot", "Mara");
    let (paragraphs, unresolved) = document.paragraphs(&substitutions);
    assert_eq!(paragraphs[0], "Confirm with Mara before \u{fffd}.");
    assert_eq!(unresolved, vec!["runway".to_owned()]);
}
