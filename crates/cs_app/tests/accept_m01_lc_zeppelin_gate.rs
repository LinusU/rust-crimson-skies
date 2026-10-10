//! #1177 (`M01-LC-ZEPPELIN-ALLEGIANCE-GATE`): the mark that stages a
//! `+0x8d` record, the lane that record carries, and M01's three team-less
//! zeppelins under them.
//!
//! Measured in the owner-supplied image (sha-256
//! `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`,
//! base `0x400000`) and recorded in
//! `docs/findings/2026-10-09-m01-lc-zeppelin-allegiance.md`:
//!
//! * `0x4a2be0` (called at `0x464838`, inside `LoadMissionData` `0x464680`,
//!   before that function's own `zeppelins.zrd` load at `0x46489c`) walks the
//!   container's node list and collects every node whose stored `unk040`
//!   carries bit 31 — the `test dword [ebx+0x28], 0x80000000` at `0x4a2cf1`.
//!   The `targets.zrd` `nodes` match it also runs (`0x4a2400`) only chooses
//!   the name kept beside the node: both paths land on the `0x71d358` insert
//!   at `0x4a2d63`..`0x4a2d8c`.
//! * `0x4a2e00` then stages one record per collected node with no further
//!   condition, writing `[rec+0x8d] = 1` at `0x4a2ec9` and `[rec+8]` from a
//!   two-bit lane of the same `unk040` (`0x4a2e3e`..`0x4a2e4b`, stored by
//!   `0x4a2570` at `0x4a259f`).
//! * `0x4bef90` reports that lane at every node it visits *before* the
//!   `ai.zrd` turret table, so what decides an answer is whether the walk
//!   meets a marked node or the turret binding first.
//!
//! The retail half reads `zbd/c1c/gamez.zbd` directly to check that order
//! over the real node array, and drives the production binding for
//! `zbd/c1c/m01` to keep #1155's three measured factions.

use std::path::{Path, PathBuf};

use cs_app::mission_world_actors::{
    ALLEGIANCE_MARKED, ALLEGIANCE_RESOLVED_CLAIM, AllegianceBinding, STAGED_TEAM_LANE_SHIFT,
    bind_mission_world_actors,
};
use cs_assets::install;
use cs_formats::gamez::{GameZNodes, RawNode, read_gamez_nodes};
use cs_formats::io::ParseContext;
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

/// The retail world container's raw node array — `0x4bef90`'s walk domain.
fn world_nodes(root: &Path, key: &str) -> GameZNodes {
    let found = install::discover(root).expect("the installation is discoverable");
    let record = found
        .manifest
        .files
        .iter()
        .find(|record| record.relative_spelling.logical_key() == key)
        .unwrap_or_else(|| panic!("no {key} in the discovered installation"));
    let host = root.join(record.relative_spelling.as_str());
    let bytes =
        std::fs::read(&host).unwrap_or_else(|error| panic!("cannot read {host:?}: {error}"));
    let mut context = ParseContext::with_defaults(key);
    read_gamez_nodes(&mut context, &bytes).expect("the node array refuses")
}

/// `0x4bef90`'s depth-first pre-order over the stored children.
fn walk(nodes: &GameZNodes, start: u32) -> Vec<&RawNode> {
    let by_index = |index: u32| nodes.nodes.iter().find(|node| node.index == index);
    let mut order = Vec::new();
    let mut stack = vec![start];
    while let Some(index) = stack.pop() {
        let Some(node) = by_index(index) else {
            continue;
        };
        if order.iter().any(|seen: &&RawNode| seen.index == index) {
            continue;
        }
        order.push(node);
        stack.extend(node.children.iter().rev());
    }
    order
}

/// M01 keeps the three factions #1155 measured, with no faction open: the
/// staged gate (#1177) never refuses here, because every marked node of
/// M01's container stages the vocabulary's `enemy` and the two zeppelins
/// whose turret binding comes first in the walk keep it.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_lc_zeppelin_gate_m01_keeps_its_three_measured_factions() {
    let root = game_dir();
    let found = install::discover(&root).expect("the installation is discoverable");
    let subject = ContentId::from_source(ContentKind::Mission, "ch1-m01").expect("id");
    let bound = bind_mission_world_actors(&root, &found, "zbd/c1c/m01", "zbd/c1c", &subject, 64);

    let expected = [
        ("piratezep", 1_i64),
        ("workersvoyagezep", 2),
        ("blackswanzep", 2),
    ];
    assert_eq!(bound.rows().len(), expected.len(), "M01 places three");
    for (row, (node, team)) in bound.rows().iter().zip(expected) {
        assert_eq!(row.node, node, "in the carrier's own order");
        assert!(
            row.team.is_none(),
            "{} still states no `team` of its own",
            row.node
        );
        match &row.allegiance {
            AllegianceBinding::Measured { team: measured, .. } => {
                assert_eq!(*measured, team, "{}'s resolver outcome", row.node);
            }
            other => panic!("{}'s allegiance is measured: {other:?}", row.node),
        }
    }

    // The acceptance: no faction stays open under the gate's claim.
    let open_factions: Vec<_> = bound
        .open_fields()
        .iter()
        .filter(|field| field.field == "faction")
        .collect();
    assert!(
        open_factions.is_empty(),
        "no record's faction refuses under the staged gate: {open_factions:?}"
    );
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
    let program = bound.program().expect("the records assemble a program");
    for actor in program.actors() {
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
    }

    // The gate's own retail facts, over the container the resolver walks.
    let nodes = world_nodes(&root, "zbd/c1c/gamez.zbd");
    let marked: Vec<&RawNode> = nodes
        .nodes
        .iter()
        .filter(|node| node.info.unk040 & ALLEGIANCE_MARKED != 0)
        .collect();
    assert!(
        !marked.is_empty(),
        "M01's container carries marked nodes for `0x4a2be0` to collect"
    );
    for node in &marked {
        assert_eq!(
            i64::from((node.info.unk040 >> STAGED_TEAM_LANE_SHIFT) & 3),
            2,
            "`{}`'s staged lane reads the vocabulary's `enemy`, so a staged record never \
             contradicts the measured factions here (unk040 {:#010x})",
            node.name,
            node.info.unk040,
        );
    }

    // …and the order the walk meets them in: the turret binding first for
    // the two zeppelins that keep it, the first marked node first for the
    // one whose staged lane decides.
    for (zeppelin, turret_node, turret_first) in [
        ("piratezep", "ctur1", true),
        ("workersvoyagezep", "utur1", false),
        ("blackswanzep", "ctur2", true),
    ] {
        let starts: Vec<_> = nodes
            .nodes
            .iter()
            .filter(|node| node.name == zeppelin)
            .map(|node| node.index)
            .collect();
        assert_eq!(starts.len(), 1, "`{zeppelin}` names one node");
        let order = walk(&nodes, starts[0]);
        let marked_at = order
            .iter()
            .position(|node| node.info.unk040 & ALLEGIANCE_MARKED != 0)
            .unwrap_or_else(|| panic!("`{zeppelin}`'s subtree holds a marked node"));
        let turret_at = order
            .iter()
            .position(|node| node.name == turret_node)
            .unwrap_or_else(|| panic!("`{turret_node}` is in `{zeppelin}`'s subtree"));
        if turret_first {
            assert!(
                turret_at < marked_at,
                "`{zeppelin}` reaches `{turret_node}` at {turret_at} before the first marked \
                 node at {marked_at}, so the turret binding keeps it"
            );
        } else {
            assert!(
                marked_at < turret_at,
                "`{zeppelin}` reaches its first marked node at {marked_at} before \
                 `{turret_node}` at {turret_at}, so `0x4bef90` reports the staged lane — \
                 which reads 2, the value the turret table would have carried"
            );
        }
    }
}
