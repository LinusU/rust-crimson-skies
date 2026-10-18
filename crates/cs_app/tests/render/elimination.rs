//! `accept_f17_f_3_` tests for task #1408: the original's back-face
//! elimination edge cases (`OWNER-STATIC-2026-10-05`).

use cs_app::render::elimination::{BFE_TOLERANCE, Elimination, EliminationReport, classify};

// Camera space: x right, y down, z into the screen; eye at the origin.
const FRONT: [[f32; 3]; 3] = [[0.0, 0.0, 1.0], [0.0, 1.0, 1.0], [1.0, 0.0, 1.0]];

fn reversed() -> Vec<[f32; 3]> {
    FRONT.iter().rev().copied().collect()
}

#[test]
fn accept_f17_f_3_classifies_front_back_edge_on_and_zero_normal() {
    assert_eq!(classify(&FRONT, false, BFE_TOLERANCE), Elimination::Front);
    assert_eq!(
        classify(&reversed(), false, BFE_TOLERANCE),
        Elimination::BackCulled
    );
    assert_eq!(
        classify(&reversed(), true, BFE_TOLERANCE),
        Elimination::BackDrawn
    );
    // Edge-on: the plane passes through the eye; two-sided does not save it.
    let edge = [[0.0, 0.0, 1.0], [0.0, 0.0, 2.0], [1.0, 0.0, 1.5]];
    assert_eq!(classify(&edge, true, BFE_TOLERANCE), Elimination::EdgeOn);
    // Collinear corners: exactly zero normal.
    let line = [[0.0, 0.0, 1.0], [0.0, 0.0, 2.0], [0.0, 0.0, 3.0]];
    assert_eq!(
        classify(&line, true, BFE_TOLERANCE),
        Elimination::ZeroNormal
    );
    assert_eq!(
        classify(&line[..2], true, BFE_TOLERANCE),
        Elimination::ZeroNormal
    );
}

#[test]
fn accept_f17_f_3_report_counts_each_elimination() {
    let mut report = EliminationReport::default();
    let edge = [[0.0, 0.0, 1.0], [0.0, 0.0, 2.0], [1.0, 0.0, 1.5]];
    for (c, two) in [
        (FRONT.to_vec(), false),
        (reversed(), false),
        (reversed(), true),
        (edge.to_vec(), true),
        (edge[..2].to_vec(), false),
    ] {
        report.record(classify(&c, two, BFE_TOLERANCE));
    }
    assert_eq!(report.front, 1);
    assert_eq!(report.back_culled, 1);
    assert_eq!(report.back_drawn, 1);
    assert_eq!(report.eliminated(), 3);
    assert_eq!(report.unreplicated(), 2);
}
