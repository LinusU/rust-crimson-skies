//! #1155: M01's team-less zeppelin records carry a measured allegiance the
//! world-actor lowering can carry.
//!
//! The retail case drives the production binding for `zbd/c1c/m01` and
//! asserts #1155's acceptance: `open_fields` no longer contains `faction`,
//! and `lower_world_actors` + `WorldActorSession::launch` succeed —
//! `MissionWorldActors::is_satisfied()` is true. Every faction is the
//! loader's measured resolver outcome (`piratezep` → `ally`,
//! `workersvoyagezep` → `enemy`, `blackswanzep` → `enemy`), bound under
//! `f34-world.zeppelin-allegiance` at `ObservedTool` and provenanced from
//! the shared `ai.zrd` span the turret binding was read from
//! (`0x4bef90` consults the `0x71d910` table that member's `TURRET`
//! records build). The test fails when the allegiance binding, the
//! lowering or the launch is removed. See
//! `docs/findings/2026-10-09-m01-lc-zeppelin-allegiance.md` for the
//! measured addresses.

use std::path::PathBuf;

use cs_app::mission_world_actors::{
    ALLEGIANCE_RESOLVED_CLAIM, TURRET_MEMBER, bind_mission_world_actors,
};
use cs_assets::install;
use cs_types::content::{ContentId, ContentKind, Resolved};

/// The installation, as the environment declares it.
fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: this retail test needs the original installation; run \
             it with `--include-ignored`"
        )
    }))
}

#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_lc_zeppelin_allegiance_m01_binds_measured_factions_and_launches() {
    let root = game_dir();
    let found = install::discover(&root).expect("the installation is discoverable");
    let subject = ContentId::from_source(ContentKind::Mission, "ch1-m01").expect("id");
    let bound = bind_mission_world_actors(&root, &found, "zbd/c1c/m01", "zbd/c1c", &subject, 64);

    // Each record's allegiance is the measured resolver outcome and names
    // its source. Two of the three bind the `ai.zrd` turret binding on a
    // node inside their own subtree (`ctur1`, `ctur2` — records 38 and 28
    // of the shared member's `TURRET` list); the third meets a marked node
    // first (#1177).
    let expected = [
        ("piratezep", 1_i64, Some("ctur1")),
        // #1177 measured the gate: `0x4a2be0` collects every bit-31 node
        // and `0x4a2e00` stages its `+0x8d` record, so `0x4bef90` reports
        // the staged lane of the `panels` node at walk position 42 — which
        // precedes `utur1` at position 77 — and that lane reads 2, the
        // `enemy` #1155 measured through the turret table.
        ("workersvoyagezep", 2, None),
        ("blackswanzep", 2, Some("ctur2")),
    ];
    assert_eq!(bound.rows().len(), expected.len(), "M01 places three");
    for (row, (node, team, turret_node)) in bound.rows().iter().zip(expected) {
        assert_eq!(row.node, node);
        assert!(
            row.team.is_none(),
            "{} still states no `team` of its own",
            row.node
        );
        match &row.allegiance {
            cs_app::mission_world_actors::AllegianceBinding::Measured {
                team: measured,
                source,
            } => {
                assert_eq!(*measured, team, "{}'s resolver outcome", row.node);
                match (turret_node, source) {
                    (
                        Some(want),
                        cs_app::mission_world_actors::AllegianceSource::TurretBinding {
                            bound_node,
                            ..
                        },
                    ) => assert_eq!(bound_node, want, "{}'s binding node", row.node),
                    (None, cs_app::mission_world_actors::AllegianceSource::MarkedNode { node }) => {
                        assert_eq!(
                            node, "panels",
                            "{} binds the staged lane of a marked node",
                            row.node
                        );
                    }
                    other => panic!(
                        "{} binds through the measured source its walk reaches first, not {other:?}",
                        row.node
                    ),
                }
            }
            other => panic!("{}'s allegiance is measured: {other:?}", row.node),
        }
    }

    // The acceptance of #1155: no faction stays open …
    let open_factions: Vec<_> = bound
        .open_fields()
        .iter()
        .filter(|field| field.field == "faction")
        .collect();
    assert!(
        open_factions.is_empty(),
        "no record's faction refuses anymore: {open_factions:?}"
    );

    // … the production lowering and the session launch both succeed …
    assert!(
        bound.lower_error().is_none(),
        "the lowering accepts the measured program: {:?}",
        bound.lower_error()
    );
    assert!(
        bound.launch_error().is_none(),
        "the session launch accepts the lowered program: {:?}",
        bound.launch_error()
    );
    assert!(
        bound.is_satisfied(),
        "MissionWorldActors::is_satisfied() is true: the session launched"
    );
    let lowered = bound.lowered().expect("the program lowered");
    assert_eq!(lowered.actors.len(), 3);
    let factions: Vec<String> = lowered
        .actors
        .iter()
        .map(|actor| actor.faction.key().to_owned())
        .collect();
    assert_eq!(
        factions,
        ["ally", "enemy", "enemy"],
        "the lowered factions are the measured vocabulary spellings"
    );

    // … and every bound faction is filed under the allegiance claim with
    // the provenance of the member its own source was read from (#1177):
    // the shared `ai.zrd` for a turret binding, the world container for a
    // staged `+0x8d` lane.
    let program = bound.program().expect("the records assemble a program");
    assert_eq!(program.actors().len(), 3);
    for (actor, row) in program.actors().iter().zip(bound.rows()) {
        let Resolved::Known(known) = &actor.faction else {
            panic!(
                "{} binds a measured faction: {:?}",
                actor.subject, actor.faction
            );
        };
        assert_eq!(
            known.provenance.claim_id.as_str(),
            ALLEGIANCE_RESOLVED_CLAIM,
            "{} is filed under the allegiance claim",
            actor.subject
        );
        assert_eq!(
            known.provenance.class,
            cs_types::evidence::ClaimStatus::ObservedTool,
            "a measured allegiance is observed_tool, never stronger"
        );
        let source = known
            .provenance
            .source
            .as_ref()
            .expect("the faction names its source span");
        match &row.allegiance {
            cs_app::mission_world_actors::AllegianceBinding::Measured {
                source: cs_app::mission_world_actors::AllegianceSource::TurretBinding { .. },
                ..
            } => {
                assert_eq!(source.member_key(), Some(TURRET_MEMBER));
                assert_eq!(
                    source.container_path(),
                    "zbd/zrdr.zbd",
                    "the turret table comes from the shared member the loader \
                     reads before `zeppelins.zrd`"
                );
            }
            cs_app::mission_world_actors::AllegianceBinding::Measured {
                source: cs_app::mission_world_actors::AllegianceSource::MarkedNode { node },
                ..
            } => {
                assert_eq!(
                    node, "panels",
                    "the staged lane is read out of a marked node's `unk040`"
                );
                assert_eq!(
                    source.member_key(),
                    None,
                    "the node array is the container's own bytes, not a member"
                );
                assert_eq!(
                    source.container_path(),
                    "zbd/c1c/gamez.zbd",
                    "the lane's provenance is the world container that stores `unk040`"
                );
            }
            other => panic!("{} names a measured source: {other:?}", row.node),
        }
    }
}
