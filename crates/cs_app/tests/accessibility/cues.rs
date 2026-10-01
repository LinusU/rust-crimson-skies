use std::collections::BTreeSet;

use cs_app::accessibility::cues::{BASE_GLYPH_PX, BASE_TEXT_PX, ObjectiveStatus, cue_for, scaled};
use cs_content::settings::{ColourFilter, MAX_UI_SCALE_PERCENT, Presentation};

#[test]
fn accept_f52_a_every_status_is_distinct_by_shape_and_text_under_every_filter() {
    for filter in ColourFilter::ALL {
        let cues: Vec<_> = ObjectiveStatus::ALL
            .into_iter()
            .map(|status| cue_for(status, filter))
            .collect();
        let shapes: BTreeSet<_> = cues.iter().map(|c| format!("{:?}", c.shape)).collect();
        let texts: BTreeSet<_> = cues.iter().map(|c| c.text_key).collect();
        assert_eq!(shapes.len(), cues.len(), "{filter:?}: shapes collide");
        assert_eq!(texts.len(), cues.len(), "{filter:?}: texts collide");
    }
}

#[test]
fn accept_f52_a_a_colour_filter_changes_only_the_colour_role() {
    for status in ObjectiveStatus::ALL {
        let off = cue_for(status, ColourFilter::Off);
        for filter in ColourFilter::ALL {
            let cue = cue_for(status, filter);
            assert_eq!((cue.shape, cue.text_key), (off.shape, off.text_key));
        }
        // With colour gone the status is still fully carried by shape and text.
        assert_eq!(cue_for(status, ColourFilter::Monochrome).colour, None);
        assert!(off.colour.is_some());
    }
}

#[test]
fn accept_f52_a_the_ui_scale_scales_cue_metrics_and_never_below_base() {
    let mut presentation = Presentation::designed();
    let base = scaled(&presentation);
    assert_eq!((base.glyph_px, base.text_px), (BASE_GLYPH_PX, BASE_TEXT_PX));
    presentation.ui_scale_percent = MAX_UI_SCALE_PERCENT;
    let large = scaled(&presentation);
    assert_eq!(large.glyph_px, BASE_GLYPH_PX * 3);
    assert_eq!(large.text_px, BASE_TEXT_PX * 3);
    presentation.ui_scale_percent = 133;
    let odd = scaled(&presentation);
    assert_eq!(odd.text_px, 19); // 14 * 1.33 = 18.62, rounded up
}
