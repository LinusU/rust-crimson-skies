//! #1178: the `world_actors` launch surface's not-satisfied detail names the
//! allegiance claim and its finding (#1155).
//!
//! `mission_launch::measure_actor_readers` builds one long `detail` when the
//! carrier decodes but the program does not lower or launch. Every claim the
//! tail explains must be findable in that text, by constant: a reader who
//! meets a faction refusal (`f34-world.zeppelin-allegiance-open`) in the
//! `still open:` list alone cannot tell *why* the record refused unless the
//! tail points at the allegiance measurement and its findings note.
//!
//! No retail mission reaches this branch today — M01's own surface is
//! `Satisfied` (#1155 settled its three team-less zeppelins) — so the detail
//! is built through the production builder `world_actor_gap_detail`, the
//! exact text `measure_actor_readers` reports. The retail closure that
//! *guards* the branch (M01 stays satisfied, `world_geometry` stays the only
//! gap) is `accept_vs_m01_runtime_` in `tests/campaign/vs_m01_runtime.rs`.
//!
//! Findings: `docs/findings/2026-10-09-m01-lc-zeppelin-allegiance.md`.

use cs_app::animation::mission::PLACEMENT_FIELDS_CLAIM;
use cs_app::mission_launch::world_actor_gap_detail;
use cs_app::mission_world_actors::{
    ALLEGIANCE_OPEN_CLAIM, ALLEGIANCE_RESOLVED_CLAIM, MOTION_RESIDUE, OpenField,
    SPAWN_ATTITUDE_CLAIM, SPAWN_POSE_CLAIM,
};
use cs_types::evidence::ClaimId;

/// The finding doc #1155 left for the allegiance measurement.
const ALLEGIANCE_FINDING: &str = "docs/findings/2026-10-09-m01-lc-zeppelin-allegiance.md";

fn open_field(claim: &str, reason: &str) -> OpenField {
    OpenField {
        actor: None,
        field: "faction",
        claim_id: ClaimId::new(claim).expect("a claim id this workspace declares"),
        reason: reason.to_owned(),
    }
}

/// A mission whose zeppelin allegiance stayed open — the shape a *different*
/// mission's launch refusal takes — names the allegiance claim, cites the
/// #1155 finding, and keeps every earlier sentence of the tail intact.
#[test]
fn accept_m01_lc_launch_detail_names_the_allegiance_claim_and_its_finding() {
    let refusal = "position/0 binds unresolved".to_owned();
    let detail = world_actor_gap_detail(
        3,
        &["piratezep".to_owned()],
        Some(&refusal as &dyn std::fmt::Display),
        None,
        &[open_field(
            ALLEGIANCE_OPEN_CLAIM,
            "a team int 7 has no measured spelling",
        )],
        &[],
        2,
    );

    // The allegiance measurement, by constant: the resolved claim and the
    // open refusal both appear, and the finding doc is cited beside them.
    assert!(
        detail.contains(ALLEGIANCE_RESOLVED_CLAIM),
        "the tail names the resolved allegiance claim: {detail}"
    );
    assert!(
        detail.contains(ALLEGIANCE_OPEN_CLAIM),
        "the open allegiance claim is named: {detail}"
    );
    assert!(
        detail.contains(ALLEGIANCE_FINDING),
        "the tail cites the #1155 finding: {detail}"
    );

    // The addition is an addition: every sentence the tail already had is
    // still there, with its own claim and findings.
    for kept in [
        SPAWN_POSE_CLAIM,
        SPAWN_ATTITUDE_CLAIM,
        PLACEMENT_FIELDS_CLAIM,
        MOTION_RESIDUE,
        "docs/findings/2026-10-08-m01-lc-world-actor-spawn.md",
        "docs/findings/2026-10-09-m01-lc-zeppelin-attitude.md",
        "docs/findings/2026-10-09-m01-lc-zeppelin-placement-position.md",
    ] {
        assert!(
            detail.contains(kept),
            "the previous tail sentence still names {kept}: {detail}"
        );
    }

    // The open field it was read from is still listed with its claim id.
    assert!(
        detail.contains(&format!("still open: faction ({ALLEGIANCE_OPEN_CLAIM})")),
        "the open field list is intact: {detail}"
    );
}

/// The allegiance sentence is part of the tail itself, not a by-product of
/// the open-field list: a detail whose open fields name only other claims
/// still explains where a faction binds and where an unsettled one refuses.
#[test]
fn accept_m01_lc_launch_detail_names_the_allegiance_claims_without_an_open_faction() {
    let detail = world_actor_gap_detail(
        1,
        &["blackswanzep".to_owned()],
        None,
        None,
        &[open_field(
            "f34c.test.some-other-claim",
            "a different open question",
        )],
        &["hookpoint (a subject that never joined)".to_owned()],
        0,
    );

    assert!(
        detail.contains(ALLEGIANCE_RESOLVED_CLAIM),
        "the tail names the resolved allegiance claim: {detail}"
    );
    assert!(
        detail.contains(ALLEGIANCE_OPEN_CLAIM),
        "the tail names the open allegiance claim: {detail}"
    );
    assert!(
        detail.contains(ALLEGIANCE_FINDING),
        "the tail cites the #1155 finding: {detail}"
    );
    assert!(
        detail.contains("still open: faction (f34c.test.some-other-claim)"),
        "the open field list carries only its own claim: {detail}"
    );
    assert!(
        !detail.contains(&format!("faction ({ALLEGIANCE_OPEN_CLAIM})")),
        "the allegiance claims reach the detail through the tail, not the open list: {detail}"
    );
}
