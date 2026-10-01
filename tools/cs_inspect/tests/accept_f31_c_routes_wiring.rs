//! Acceptance scenarios F31-C for the producer-to-consumer wiring: the
//! declared content route projects onto the runtime graph the follower
//! consumes, a moving anchor is bound (or refused), and the `routes --follow`
//! command drives the production follower through a displaced rejoin, a
//! teardown and a fresh-session retry.
//!
//! Spec: `specs/F31-ai-navigation-routes-and-obstacle-avoidance.md`, stage
//! `### F31-C`. Task test prefix: `accept_f31_c_`.
//!
//! These tests drive production code only: [`project_route`] and the
//! `cs-inspect` `routes` command, over `cs_content::routes` and
//! `cs_sim::ai::navigation`. Removing the projection, mis-mapping a node, or
//! making an unknown default instead of error makes one of them fail.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data.

use std::process::Command;

use cs_content::routes::{
    AnchorKind, MovingAnchor, ReferenceFrame, RouteDefinition, RouteDraft, RouteNode,
    RouteTermination, declared_synthetic_arch_route,
};
use cs_inspect::routes::{
    AnchorBinding, RouteProjectionError, project_route, routes_command_result,
};
use cs_sim::ai::navigation::{
    RouteFrame, RouteNodeId, RouteTermination as NavRouteTermination, synthetic_arch_route,
};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("valid claim id")
}

fn designed() -> Provenance {
    Provenance::designed(claim("f31c.integration"))
}

fn anchor() -> ContentId {
    ContentId::from_source(ContentKind::SceneNode, "synthetic.moving.anchor")
        .expect("valid anchor id")
}

fn world_draft(nodes: Vec<RouteNode>, termination: RouteTermination) -> RouteDraft {
    RouteDraft {
        id: ContentId::from_source(ContentKind::Route, "synthetic.c.world")
            .expect("valid route id"),
        origin: Origin::SyntheticFixture,
        frame: ReferenceFrame::World,
        termination,
        clearance_m: Resolved::Known(Known::new(2.0, designed())),
        nodes,
        edges: Vec::new(),
        provenance: designed(),
    }
}

fn moving_draft(nodes: Vec<RouteNode>, termination: RouteTermination) -> RouteDraft {
    RouteDraft {
        frame: ReferenceFrame::Moving(MovingAnchor {
            anchor: anchor(),
            kind: AnchorKind::Carrier,
        }),
        ..world_draft(nodes, termination)
    }
}

/// The declared content fixture projects byte-for-byte onto the runtime
/// fixture the follower consumes, so the producer and the consumer agree.
#[test]
fn accept_f31_c_projection_reproduces_the_runtime_arch_fixture() {
    let resolved = declared_synthetic_arch_route()
        .resolve()
        .expect("the declared fixture resolves");
    let projected = project_route(&resolved, &[]).expect("the world route projects");

    assert_eq!(
        projected,
        synthetic_arch_route(),
        "the projected content route must equal the runtime fixture"
    );
}

/// A moving route projects only when its authored anchor is bound to a runtime
/// actor id; an unbound anchor is refused by name, never addressed at an
/// invented id.
#[test]
fn accept_f31_c_projection_binds_a_moving_anchor_or_refuses_it() {
    let nodes = declared_synthetic_arch_route().nodes().to_vec();
    let definition = RouteDefinition::try_new(moving_draft(nodes, RouteTermination::End))
        .expect("the moving route is valid");
    let resolved = definition.resolve().expect("the moving route resolves");

    assert_eq!(
        project_route(&resolved, &[]),
        Err(RouteProjectionError::UnboundAnchor {
            anchor: anchor().as_str().to_owned()
        })
    );

    let projected = project_route(
        &resolved,
        &[AnchorBinding {
            anchor: anchor(),
            runtime_id: 99,
        }],
    )
    .expect("the bound moving route projects");
    assert_eq!(projected.frame, RouteFrame::Moving { anchor: 99 });
    assert_eq!(projected.nodes.len(), 5);
}

/// A declared loop termination reaches the runtime graph as a loop, so the
/// follower re-arms the marker sequence instead of the projection refusing the
/// record or silently following it as an end.
///
/// The refusal this test used to pin was removed by task #447, which gave the
/// runtime explicit loop semantics (`accept_t447_` in `cs_sim`). Whether the
/// original 2000 route encoding expresses a loop is still unmeasured (F13;
/// F31-D owns retail coverage); this is project design, not an original claim.
#[test]
fn accept_f31_c_projection_carries_a_loop_termination_into_the_runtime_graph() {
    let nodes = declared_synthetic_arch_route().nodes().to_vec();
    let definition = RouteDefinition::try_new(world_draft(nodes, RouteTermination::Loop))
        .expect("the looping route is valid content");
    let resolved = definition.resolve().expect("the looping route resolves");

    let projected =
        project_route(&resolved, &[]).expect("the looping route projects into the runtime graph");
    assert_eq!(
        projected.termination(),
        NavRouteTermination::Loop,
        "the declared loop must survive projection, not be defaulted to an end"
    );
    assert_eq!(projected.nodes.len(), definition.nodes().len());

    // The ending fixture still projects as an end, so the mapping is a real
    // discrimination and not a constant.
    let ending = declared_synthetic_arch_route()
        .resolve()
        .expect("the declared fixture resolves");
    let projected_end = project_route(&ending, &[]).expect("the world route projects");
    assert_eq!(projected_end.termination(), NavRouteTermination::End);
}

/// The runtime graph keys a projected node by its authored sequence, so a
/// container reorder cannot rename a marker.
#[test]
fn accept_f31_c_projection_keys_nodes_by_authored_sequence() {
    let target = declared_synthetic_arch_route()
        .nodes()
        .iter()
        .find(|node| node.id.as_str() == "arch")
        .expect("the arch node exists")
        .sequence;
    let resolved = declared_synthetic_arch_route()
        .resolve()
        .expect("the declared fixture resolves");
    let projected = project_route(&resolved, &[]).expect("the world route projects");
    let arch = projected
        .nodes
        .iter()
        .find(|node| node.sequence == target)
        .expect("the arch node projects");
    assert_eq!(arch.id, RouteNodeId(target));
    assert!(arch.mandatory);
}

/// The `routes --follow` command drives the production follower end to end: a
/// displaced actor rejoins before its next mandatory marker, the roster is torn
/// down and a fresh session retries from scratch, refusing a stale command
/// generation.
#[test]
fn accept_f31_c_follow_command_reports_rejoin_teardown_and_retry() {
    let run = routes_command_result(&["--follow".to_owned()]);
    assert_eq!(run.exit_code, 0, "{:?}", run.diagnostics);

    let summary = run.follow.expect("the follow report declares its counts");
    assert_eq!(summary.nodes, 5);
    assert!(summary.rejoined, "the displaced actor must rejoin");
    assert!(summary.rejoin_tick.is_some());
    assert!(summary.torn_down, "the actor roster is torn down");
    assert!(summary.retry_reset, "a fresh session retries from scratch");
    assert!(
        summary.stale_generation_refused,
        "a stale command generation is refused"
    );

    let report = run.report.expect("the follow report is produced");
    assert!(report.contains("\"schema\":\"cs-inspect-routes-follow/v1\""));
    assert!(report.contains("\"source\":\"synthetic-fixture\""));
    assert!(report.contains("\"retail\":false"));
    assert!(report.contains("\"route\":{\"id\":\"route/synthetic.arch\""));
    assert!(
        report.contains("\"rejoined\":true"),
        "the report records the rejoin: {report}"
    );
    assert!(
        report.contains("\"torn_down\":true")
            && report.contains("\"retry_reset\":true")
            && report.contains("\"stale_generation_refused\":true"),
        "the report records the lifecycle: {report}"
    );
}

/// The shipped binary runs `routes --follow` too, and a bad flag is still
/// invalid input with a nonzero exit code.
#[test]
fn accept_f31_c_follow_binary_runs_and_refuses_bad_input() {
    let output = Command::new(env!("CARGO_BIN_EXE_cs-inspect"))
        .args(["routes", "--follow"])
        .output()
        .expect("the cs-inspect binary must run");
    assert_eq!(
        output.status.code(),
        Some(0),
        "routes --follow must exit zero"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"schema\":\"cs-inspect-routes-follow/v1\"")
            && stdout.contains("\"rejoined\":true"),
        "the binary prints the follow report, got: {stdout:?}"
    );

    let refused = Command::new(env!("CARGO_BIN_EXE_cs-inspect"))
        .args(["routes", "--nope"])
        .output()
        .expect("the cs-inspect binary must run");
    assert_eq!(refused.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("cs-inspect routes:"));
}
