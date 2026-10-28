//! Task #2177 (T452-REG-PER-BOX-WEIGHT): the measured 480 weight / 410 cost
//! per enabled rocket box has an owner that sums an airframe record's two box
//! counts, and the weight unit stays unmeasured as kilograms.
//!
//! Task test prefix: `accept_t452_per_box_`.

use cs_content::ordnance::{
    ORIGINAL_HARDPOINT_COST_PER_BOX, ORIGINAL_HARDPOINT_WEIGHT_PER_BOX,
    ORIGINAL_PLANE_TOTAL_BOX_FACTORS, OriginalBoxCountError, OriginalHardpointBoxes,
    OriginalWeightUnits, original_hardpoint_price,
};

#[test]
fn accept_t452_per_box_totals_match_the_measured_total_path() {
    let [w1, w2, w3] = ORIGINAL_PLANE_TOTAL_BOX_FACTORS.0;
    let [c1, c2, c3] = ORIGINAL_PLANE_TOTAL_BOX_FACTORS.1;
    assert_eq!(w1 * w2 * w3, ORIGINAL_HARDPOINT_WEIGHT_PER_BOX);
    assert_eq!(c1 * c2 * c3, ORIGINAL_HARDPOINT_COST_PER_BOX);
    let price = original_hardpoint_price();
    for a in 0..=4 {
        for b in 0..=4 {
            let boxes = OriginalHardpointBoxes::try_new([a, b]).unwrap();
            assert_eq!(boxes.total_boxes(), a + b);
            assert_eq!(boxes.weight(), OriginalWeightUnits((a + b) * w1 * w2 * w3));
            assert_eq!(boxes.cost(), (a + b) * c1 * c2 * c3);
            // The totalizer equals the sum of the two per-side purchase rows.
            assert_eq!(boxes.weight().0, price.weight(a) + price.weight(b));
            assert_eq!(boxes.cost(), price.cost(a) + price.cost(b));
        }
    }
    let full = OriginalHardpointBoxes::try_new([4, 4]).unwrap();
    assert_eq!((full.weight().0, full.cost()), (3840, 3280));
}

#[test]
fn accept_t452_per_box_refuses_more_boxes_than_a_side_holds() {
    assert_eq!(
        OriginalHardpointBoxes::try_new([2, 5]),
        Err(OriginalBoxCountError { side: 1, count: 5 })
    );
}
