//! The fit-or-scroll layout of a localized string in a panel with required
//! buttons (F51-A, AC01).
//!
//! The panel here is 640x480 with an `Ok` and a `Cancel` button in the bottom
//! 40 pixels. The minimum scenario is that a long translation in any of the
//! declared fixture locales either fits or scrolls **inside the free band
//! above the buttons**, and never paints over them.

use bevy::math::Rect;
use cs_app::text::layout::{
    ControlIdError, LayoutDiagnostic, LayoutError, LayoutRequest, RequiredControl, TextFit,
    layout_text,
};
use cs_app::text::synthetic_monospace;
use cs_content::localization::{
    SYNTHETIC_LONG_TRANSLATION_KEY, SubstitutionTable, TextResolution,
    declared_synthetic_text_catalog,
};

use crate::common::{
    BUTTON_ROW_TOP, PANEL, buttons, chain, control_id, document, request, substitutions, text_id,
};

/// The panel the text is allowed in: everything above the button row.
fn expected_viewport() -> Rect {
    Rect::new(PANEL.min.x, PANEL.min.y, PANEL.max.x, BUTTON_ROW_TOP)
}

/// AC01: a long localized text fits or scrolls without covering the required
/// buttons, in every locale the fixture declares.
#[test]
fn accept_f51_a_long_translation_scrolls_clear_of_the_required_buttons() {
    let catalog = declared_synthetic_text_catalog();
    let id = text_id(SYNTHETIC_LONG_TRANSLATION_KEY);
    let metrics = synthetic_monospace(16.0);
    let values = substitutions();
    let controls = buttons();

    // The same screen, three locales of different length: the English briefing
    // fits the band, the two longer translations do not. Neither outcome may
    // touch a button.
    for (selected, expect_scrolling) in [("en-us", false), ("de-de", true), ("fr-fr", true)] {
        let TextResolution::Resolved { row, .. } = catalog.resolve(&id, &chain(selected, &[]))
        else {
            panic!("{selected} has its own row");
        };
        let document = document(row.text());
        let layout = layout_text(&request(
            Some(id.clone()),
            &document,
            &metrics,
            &values,
            &controls,
        ))
        .expect("the panel has a free band");

        // The viewport is the strip above the buttons, not the whole panel.
        assert_eq!(layout.viewport(), expected_viewport(), "{selected}");
        assert_eq!(layout.panel(), PANEL, "{selected}");

        let viewport_height = BUTTON_ROW_TOP;
        let lines = layout.fit().lines();
        assert!(lines > 1, "{selected}");
        assert_eq!(layout.fit().is_scrolling(), expect_scrolling, "{selected}");
        match layout.fit() {
            TextFit::Fits { lines } => {
                assert!(!expect_scrolling, "{selected}");
                assert_eq!(layout.max_scroll(), 0.0, "{selected}");
                assert_eq!(lines, layout.visible_line_indices().len(), "{selected}");
            }
            TextFit::Scrolls {
                lines,
                hidden_lines,
                content_height,
                viewport_height: band,
            } => {
                assert!(expect_scrolling, "{selected}");
                assert!(hidden_lines > 0, "{selected}");
                assert!(content_height > band, "{selected}");
                assert_eq!(band, viewport_height, "{selected}");
                assert!(
                    lines - hidden_lines == layout.visible_line_indices().len(),
                    "{selected}"
                );
            }
        }
        assert_eq!(viewport_height, BUTTON_ROW_TOP, "{selected}");

        // The observable failure this whole slice exists to prevent: no painted
        // line may touch a required button.
        for control in &controls {
            assert!(
                !layout.covers(control),
                "{selected}: the layout painted over {}",
                control.id()
            );
        }
        // Nor may any painted line box leave the viewport.
        for index in layout.visible_line_indices() {
            let painted = layout
                .painted_rect(index)
                .unwrap_or_else(|| panic!("{selected}: line {index} has a painted box"));
            assert!(painted.min.y >= PANEL.min.y, "{selected}: line {index}");
            assert!(painted.max.y <= BUTTON_ROW_TOP, "{selected}: line {index}");
            assert!(
                painted.height() <= layout.line_height() + 0.01,
                "{selected}: line {index}"
            );
        }
        // Every line of the source text survived: nothing was truncated away.
        let laid_out: String = layout.text().replace('\n', " ");
        for word in row.text().split_whitespace() {
            assert!(
                laid_out.contains(word),
                "{selected}: {word:?} vanished from the layout"
            );
        }
    }
}

/// The same screen with a short string: it fits, and it still does not touch the
/// buttons. AC01 is "fits **or** scrolls", so both halves are checked.
#[test]
fn accept_f51_a_a_short_translation_fits_without_touching_the_buttons() {
    let catalog = declared_synthetic_text_catalog();
    let id = text_id("mission.briefing.confirm");
    let metrics = synthetic_monospace(16.0);
    let values = substitutions();
    let controls = buttons();

    let TextResolution::Resolved { row, .. } = catalog.resolve(&id, &chain("en-us", &[])) else {
        panic!("the confirm row exists");
    };
    let document = document(row.text());
    let layout =
        layout_text(&request(Some(id), &document, &metrics, &values, &controls)).expect("layout");

    assert_eq!(layout.fit(), TextFit::Fits { lines: 1 });
    assert!(layout.fit().is_fits());
    assert!(!layout.fit().is_scrolling());
    assert_eq!(layout.max_scroll(), 0.0);
    assert_eq!(layout.lines().len(), 1);
    assert_eq!(layout.lines()[0].text(), "Confirm your corridor");
    assert_eq!(layout.visible_line_indices(), vec![0]);
    for control in &controls {
        assert!(!layout.covers(control), "the layout painted over a button");
    }
}

/// Scrolling is what keeps a long translation usable: a focus step scrolls the
/// wanted line fully into the viewport and never past the end of the text, so a
/// line can never end up under a button while it is being read.
#[test]
fn accept_f51_a_scrolling_keeps_the_focused_line_inside_the_viewport() {
    let catalog = declared_synthetic_text_catalog();
    let id = text_id(SYNTHETIC_LONG_TRANSLATION_KEY);
    let metrics = synthetic_monospace(16.0);
    let values = substitutions();
    let controls = buttons();
    let TextResolution::Resolved { row, .. } = catalog.resolve(&id, &chain("de-de", &[])) else {
        panic!("the german row exists");
    };
    let document = document(row.text());
    let layout = layout_text(&request(Some(id), &document, &metrics, &values, &controls))
        .expect("the panel has a free band");
    let line_count = layout.lines().len();
    assert!(line_count > 4, "the scenario needs several lines");
    assert!(layout.max_scroll() > 0.0);

    // The first line needs no scrolling, the last line needs all of it, and
    // every offset stays inside the legal range.
    assert_eq!(layout.scroll_offset_for_line(0), 0.0);
    assert_eq!(
        layout.scroll_offset_for_line(line_count - 1),
        layout.max_scroll()
    );
    for index in 0..line_count {
        let offset = layout.scroll_offset_for_line(index);
        assert!(
            (0.0..=layout.max_scroll()).contains(&offset),
            "line {index} scrolled to {offset}, outside 0..={}",
            layout.max_scroll()
        );
        // The scrolled line is fully inside the viewport at that offset.
        let top = layout.lines()[index].rect().min.y - layout.viewport().min.y - offset;
        let bottom = top + layout.line_height();
        assert!(top >= -0.001, "line {index} starts above the viewport");
        assert!(bottom <= layout.viewport().height() + 0.001, "line {index}");
    }
    // An out-of-range focus target is clamped, never a scroll past the text.
    assert_eq!(
        layout.scroll_offset_for_line(line_count + 10),
        layout.max_scroll()
    );
    assert_eq!(
        layout.scroll_offset_for_line(usize::MAX),
        layout.max_scroll()
    );
}

/// A UI scale change re-measures the same text, and the reserved buttons stay
/// clear at every scale.
#[test]
fn accept_f51_a_ui_scale_changes_the_wrap_but_never_the_buttons() {
    let catalog = declared_synthetic_text_catalog();
    let id = text_id(SYNTHETIC_LONG_TRANSLATION_KEY);
    let values = substitutions();
    let controls = buttons();
    let TextResolution::Resolved { row, .. } = catalog.resolve(&id, &chain("en-us", &[])) else {
        panic!("the english row exists");
    };
    let document = document(row.text());

    let small = synthetic_monospace(16.0);
    let large = small.scaled(2.0).expect("a finite positive scale is valid");
    assert_eq!(large.pixel_size(), 32.0);
    assert_eq!(large.line_height(), small.line_height() * 2.0);
    // Scaling adds no glyphs: a missing glyph cannot become a covered one.
    assert!(!large.covers('ü'));

    let mut line_counts = Vec::new();
    for metrics in [&small, &large] {
        let layout = layout_text(&request(
            Some(id.clone()),
            &document,
            metrics,
            &values,
            &controls,
        ))
        .expect("the panel has a free band");
        assert_eq!(layout.viewport(), expected_viewport());
        for control in &controls {
            assert!(!layout.covers(control), "a button was covered at scale");
        }
        line_counts.push(layout.lines().len());
    }
    assert!(
        line_counts[1] > line_counts[0],
        "twice the size must need more lines: {line_counts:?}"
    );
}

/// A panel with no free band is a screen bug, and it is reported rather than
/// painted over a control.
#[test]
fn accept_f51_a_a_panel_without_a_free_band_is_refused() {
    let metrics = synthetic_monospace(16.0);
    let values = substitutions();
    let document = document("Confirm your corridor");

    let covering =
        vec![RequiredControl::new(control_id("dialog.overlay"), PANEL).expect("a ui_resource id")];
    assert_eq!(
        layout_text(&request(None, &document, &metrics, &values, &covering))
            .expect_err("a control covering the whole panel leaves no free band"),
        LayoutError::NoFreeBand
    );

    // A control that only covers the bottom of the panel leaves a free band.
    let top_control = vec![
        RequiredControl::new(
            control_id("dialog.banner"),
            Rect::new(0.0, 0.0, 640.0, 200.0),
        )
        .expect("a ui_resource id"),
    ];
    let layout = layout_text(&request(None, &document, &metrics, &values, &top_control))
        .expect("the strip below the banner is free");
    assert_eq!(layout.viewport().min.y, 200.0);
    assert_eq!(layout.viewport().max.y, 480.0);

    // An empty panel is refused too.
    let empty = request(None, &document, &metrics, &values, &[]);
    let empty_request = LayoutRequest {
        panel: Rect::new(10.0, 10.0, 10.0, 10.0),
        ..empty
    };
    assert_eq!(
        layout_text(&empty_request).expect_err("an empty panel is refused"),
        LayoutError::EmptyPanel
    );
}

/// The AC01 path reports what it found: the refused control markup, the missing
/// glyphs of a translation the font cannot draw, and an unresolved substitution
/// are all diagnostics, so a screen can show them instead of silently dropping
/// them.
#[test]
fn accept_f51_a_the_layout_reports_missing_glyphs_and_refused_markup() {
    let catalog = declared_synthetic_text_catalog();
    let id = text_id(SYNTHETIC_LONG_TRANSLATION_KEY);
    let metrics = synthetic_monospace(16.0);
    let controls = buttons();

    let TextResolution::Resolved { row, .. } = catalog.resolve(&id, &chain("de-de", &[])) else {
        panic!("the german row exists");
    };
    // The German fixture string carries umlauts the declared ASCII coverage
    // cannot draw, and one control the grammar does not admit.
    let source = format!("{}[blink]x[/blink]", row.text());
    let document = document(&source);
    let values = substitutions();
    let layout = layout_text(&request(Some(id), &document, &metrics, &values, &controls))
        .expect("the panel has a free band");

    let missing: Vec<(char, usize, usize)> = layout
        .diagnostics()
        .iter()
        .filter_map(|diagnostic| match diagnostic {
            LayoutDiagnostic::MissingGlyph {
                ch,
                line,
                occurrences,
            } => Some((*ch, *line, *occurrences)),
            _ => None,
        })
        .collect();
    assert!(
        missing.iter().any(|(ch, _, _)| *ch == 'ü'),
        "the umlauts must be counted: {missing:?}"
    );
    assert!(missing.iter().all(|(_, _, occurrences)| *occurrences > 0));
    for (ch, line, _) in &missing {
        assert!(
            layout.lines()[*line].text().contains(*ch),
            "'{ch}' is reported on a line that does not contain it"
        );
    }
    assert!(
        layout
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "unknown_tag")
    );
    // The refused control is still displayed, verbatim.
    assert!(layout.text().contains("[blink]"));
    for control in &controls {
        assert!(!layout.covers(control));
    }
}

/// A substitution the screen did not supply renders as a visible marker, and
/// the layout names it, so a long translated briefing never shows a silent
/// blank where a pilot's name should be.
#[test]
fn accept_f51_a_an_unresolved_substitution_is_reported_by_the_layout() {
    let metrics = synthetic_monospace(16.0);
    let values = SubstitutionTable::new();
    let controls = buttons();
    let document = document("Corridor {runway} confirmed by {pilot}.");
    let layout = layout_text(&request(None, &document, &metrics, &values, &controls))
        .expect("the panel has a free band");

    let unresolved: Vec<(&str, usize)> = layout
        .diagnostics()
        .iter()
        .filter_map(|diagnostic| match diagnostic {
            LayoutDiagnostic::UnresolvedSubstitution { id, line } => {
                Some((id.as_str(), line.expect("the marker landed on a line")))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        unresolved.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        vec!["runway", "pilot"],
        "each id is named once, in document order"
    );
    // Both markers are on the same line here, and it is the line that really
    // holds them: a diagnostic must not blame a line that has no marker.
    for (id, line) in &unresolved {
        assert!(
            layout.lines()[*line].text().contains('\u{fffd}'),
            "{id} is reported on line {line}, which holds no marker: {:?}",
            layout.lines()[*line].text()
        );
    }
    assert!(layout.text().contains('\u{fffd}'));
    for control in &controls {
        assert!(!layout.covers(control));
    }
}

/// Two unresolved substitutions on different lines are each reported on *their
/// own* line. Attributing every one of them to the first line that happens to
/// hold a marker sends a content fix to the wrong place, which is the whole
/// point of reporting a line at all.
#[test]
fn accept_f51_a_each_unresolved_substitution_is_reported_on_its_own_line() {
    let metrics = synthetic_monospace(16.0);
    let values = SubstitutionTable::new();
    let controls = buttons();
    // A hard line break puts the two markers in different paragraphs, and each
    // paragraph is long enough to wrap so the markers end up on different
    // lines.
    let filler = "transmission ".repeat(40);
    let source = format!("{filler}{{runway}} now\n{filler}{{pilot}} now");
    let document = document(&source);
    let layout = layout_text(&request(None, &document, &metrics, &values, &controls))
        .expect("the panel has a free band");

    let reported: Vec<(String, Option<usize>)> = layout
        .diagnostics()
        .iter()
        .filter_map(|diagnostic| match diagnostic {
            LayoutDiagnostic::UnresolvedSubstitution { id, line } => Some((id.clone(), *line)),
            _ => None,
        })
        .collect();
    assert_eq!(reported.len(), 2, "{reported:?}");

    let lines: Vec<usize> = reported
        .iter()
        .map(|(_, line)| line.expect("the marker landed on a line"))
        .collect();
    assert_ne!(
        lines[0], lines[1],
        "the two markers are on different lines, so they must be reported separately: {reported:?}"
    );
    for (id, line) in &reported {
        let line = line.expect("the marker landed on a line");
        assert!(
            layout.lines()[line].text().contains('\u{fffd}'),
            "{id} is reported on line {line}, which holds no marker"
        );
        // The line is inside the text that was laid out, and the id is the one
        // the marker really stood for.
        assert!(line < layout.lines().len());
    }
    // The order is document order, and each id is named exactly once.
    assert_eq!(
        reported
            .iter()
            .map(|(id, _)| id.as_str())
            .collect::<Vec<_>>(),
        vec!["runway", "pilot"]
    );
}

/// A single token wider than the band is broken by character instead of
/// overflowing, so even an unbreakable word cannot paint outside the viewport.
#[test]
fn accept_f51_a_a_token_wider_than_the_band_is_broken_not_overflowed() {
    let metrics = synthetic_monospace(16.0);
    let values = substitutions();
    let controls = buttons();
    let document = document(&"x".repeat(2000));
    let layout = layout_text(&request(None, &document, &metrics, &values, &controls))
        .expect("the panel has a free band");

    assert!(
        layout
            .diagnostics()
            .iter()
            .any(|diagnostic| matches!(diagnostic, LayoutDiagnostic::BrokenWord { .. }))
    );
    assert!(layout.fit().is_scrolling());
    let visible = layout.visible_line_indices();
    assert!(!visible.is_empty());
    assert!(
        visible.len() < layout.lines().len(),
        "a 2000-character token cannot fit, so some lines scroll"
    );
    for index in visible {
        let painted = layout
            .painted_rect(index)
            .expect("a visible line has a painted box");
        assert!(painted.max.y <= BUTTON_ROW_TOP);
        assert_eq!(painted.min.x, expected_viewport().min.x);
        assert_eq!(painted.max.x, expected_viewport().max.x);
    }
    // The whole token is still there, only broken across lines.
    assert_eq!(
        layout.text().replace('\n', ""),
        "x".repeat(2000),
        "breaking a token must not drop characters"
    );
    for control in &controls {
        assert!(!layout.covers(control));
    }
}

/// `covers` is a real check, not a tautology: the free band is computed to
/// exclude the declared controls, so the AC01 assertion above is guaranteed by
/// construction *unless* the geometry check itself works. A control placed
/// inside the viewport — one the layout was not told about — is detected, so a
/// regression in the painted geometry cannot hide behind the free band.
#[test]
fn accept_f51_a_covers_detects_a_control_inside_the_viewport() {
    let metrics = synthetic_monospace(16.0);
    let values = substitutions();
    let document = document("A briefing line long enough to occupy the band.");
    // No declared controls, so the viewport is the whole panel.
    let layout = layout_text(&request(None, &document, &metrics, &values, &[]))
        .expect("an unreserved panel has a free band");
    assert_eq!(layout.viewport(), PANEL);

    // A control on the panel edge, outside every painted line, is clear.
    let below = RequiredControl::new(
        control_id("dialog.status"),
        Rect::new(0.0, 470.0, 640.0, 480.0),
    )
    .expect("a ui_resource id");
    assert!(
        !layout.covers(&below),
        "a control below the text must not be reported"
    );

    // A control over the first line's box is detected.
    let over_first_line =
        RequiredControl::new(control_id("dialog.banner"), layout.lines()[0].rect())
            .expect("a ui_resource id");
    assert!(
        layout.covers(&over_first_line),
        "a control on a painted line must be reported as covered"
    );

    // And in the declared-control case the same geometry proves the real
    // buttons are clear, because they sit in the band the layout excluded.
    let controls = buttons();
    let reserved = layout_text(&request(None, &document, &metrics, &values, &controls))
        .expect("the panel has a free band");
    assert_eq!(reserved.viewport().max.y, BUTTON_ROW_TOP);
    for control in &controls {
        assert!(!reserved.covers(control));
    }
    // Moving a button up into the band changes the band, and the layout then
    // cannot paint the line the button sits on.
    let intruding = vec![
        RequiredControl::new(control_id("dialog.ok"), Rect::new(160.0, 20.0, 300.0, 60.0))
            .expect("a ui_resource id"),
    ];
    let narrowed =
        layout_text(&request(None, &document, &metrics, &values, &intruding)).expect("a free band");
    assert!(
        narrowed.viewport().min.y >= 60.0,
        "{:?}",
        narrowed.viewport()
    );
    assert!(!narrowed.covers(&intruding[0]));
    for index in 0..narrowed.lines().len() {
        let painted = narrowed
            .painted_rect(index)
            .expect("a laid-out line has a painted box");
        assert!(
            painted.intersect(intruding[0].rect()).is_empty(),
            "line {index} painted over the intruding button: {painted:?}"
        );
    }
}

/// A required control keeps a `ui_resource` identity, so a locale-dependent
/// lookup can never be pointed at mission or save identity.
#[test]
fn accept_f51_a_a_required_control_must_be_a_ui_resource() {
    use cs_types::content::{ContentId, ContentKind};

    let rect = Rect::new(0.0, 0.0, 10.0, 10.0);
    assert!(
        RequiredControl::new(
            ContentId::from_source(ContentKind::UiResource, "dialog.ok").expect("valid"),
            rect
        )
        .is_ok()
    );
    assert_eq!(
        RequiredControl::new(
            ContentId::from_source(ContentKind::Mission, "m01").expect("valid"),
            rect,
        )
        .expect_err("a mission is not a button"),
        ControlIdError::NotAUiResource {
            kind: ContentKind::Mission
        }
    );
}
