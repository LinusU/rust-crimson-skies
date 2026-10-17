//! Task #540 acceptance tests: the subtree content each declared cockpit
//! binding names, resolved through `cs_app::camera::coverage`.
//!
//! Spec: `specs/F21-cameras-cockpit-views-and-spyglass.md`, stage `### F21-D`;
//! shared contract `docs/contracts/UI-NETWORK.md`. Task test prefix:
//! `accept_t540_`.
//!
//! F21-D's coverage audit answers *which node* a declared binding resolves to.
//! This task answers *what that node's own subtree draws*: the instrument under
//! test is the production [`resolve_cockpit_content`] pass, driven end to end —
//! `discover_cockpit_bindings` over an authored `support\cockpit.gw`,
//! `audit_cockpit_coverage` over an authored node array, then the content pass
//! over the same report and the same array. No test carries its own subtree
//! walk; the authored arrays below put a `NoMesh` container over drawable
//! descendants, one ambiguous name under two subtrees, one `Bound` leaf, one
//! `Absent` name and a `NoMesh` leaf so every path of the pass is exercised.
//!
//! The retail test repeats the measurement over `$CS_GAME_DIR` and pins the
//! numbers the task measured from `ZBD/planes.zbd`: twenty declared bindings
//! over eleven airframes, forty-four pairs the audit leaves unresolved, and
//! every one of the two hundred twenty resolved to drawable content.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use cs_app::camera::coverage::{
    CockpitAirframe, CockpitContentReport, CockpitCoverageError, CockpitNodeCoverage,
    audit_cockpit_coverage, discover_cockpit_bindings, resolve_cockpit_content,
};
use cs_content::scene::discover_airframe_roster;
use cs_formats::gamez::GameZNodes;
use cs_types::content::{ContentId, ContentKind};
use cs_types::evidence::ContentHash;

use crate::camera::coverage::{
    FIXTURE, Node, RETAIL_AIRFRAMES, RETAIL_COCKPIT_BINDINGS, airframe, archive_id, cockpit_claim,
    container, coverage_state, decode, line, observed, quote, raw_node, retail_cockpit_claim,
    retail_dir, retail_provenance, retail_roster_declarations, synthetic_archive,
};

// ------------------------------------------------------------- fixtures ---

/// The authored `support\cockpit.gw` these tests walk: the airframe variable,
/// then five names — a container, an ambiguous name, another container, a
/// leaf and a name no airframe stores, in the order the script spells them.
fn content_cockpit_script() -> Vec<u8> {
    container(&[(
        b"support\\cockpit.gw",
        vec![
            line(&[b"FindNode", b"%player_plane%"]),
            line(&[b"FindSubNode", b"gungauge"]),
            line(&[b"FindSubNode", b"4char_ammo"]),
            line(&[b"FindSubNode", b"missilegauge"]),
            line(&[b"FindSubNode", b"taildamage"]),
            line(&[b"FindSubNode", b"absent_node"]),
        ],
    )])
}

/// The authored airframe archive the content tests measure.
///
/// Two airframes, both owned by a `world1` node whose parent claim each root
/// returns — the shape that makes "the walk follows children only" observable:
/// any ascent would pull the other aircraft's nodes into this one's rows.
///
/// ```text
/// world1 (13)
/// ├─ player_kestrel (0)
/// │  ├─ gungauge (1) mesh -1 ─┬─ ggpanel (5) mesh 40
/// │  │                        ├─ ggarrow (6) mesh 44
/// │  │                        └─ markers (7) mesh -1
/// │  ├─ 4char_ammo (2) mesh 42
/// │  ├─ missilegauge (3) mesh -1 ── 4char_ammo (8) mesh 45
/// │  └─ taildamage (4) mesh 43
/// └─ player_warhawk (9)
///    ├─ gungauge (10) mesh -1 ── needle (12) mesh 50
///    └─ 4char_ammo (11) mesh -1
/// ```
///
/// Kestrel's `4char_ammo` is genuinely ambiguous (nodes 2 and 8), and node 8
/// sits inside `missilegauge`'s subtree as well — two declared bindings' rows
/// legitimately name the same geometry, which is what the per-airframe union
/// deduplicates.
fn content_archive() -> GameZNodes {
    let specs = [
        Node {
            name: "player_kestrel",
            parent: Some(13),
            children: vec![1, 2, 3, 4],
            mesh_index: -1,
        },
        Node {
            name: "gungauge",
            parent: Some(0),
            children: vec![5, 6, 7],
            mesh_index: -1,
        },
        Node {
            name: "4char_ammo",
            parent: Some(0),
            children: Vec::new(),
            mesh_index: 42,
        },
        Node {
            name: "missilegauge",
            parent: Some(0),
            children: vec![8],
            mesh_index: -1,
        },
        Node {
            name: "taildamage",
            parent: Some(0),
            children: Vec::new(),
            mesh_index: 43,
        },
        Node {
            name: "ggpanel",
            parent: Some(1),
            children: Vec::new(),
            mesh_index: 40,
        },
        Node {
            name: "ggarrow",
            parent: Some(1),
            children: Vec::new(),
            mesh_index: 44,
        },
        Node {
            name: "markers",
            parent: Some(1),
            children: Vec::new(),
            mesh_index: -1,
        },
        Node {
            name: "4char_ammo",
            parent: Some(3),
            children: Vec::new(),
            mesh_index: 45,
        },
        Node {
            name: "player_warhawk",
            parent: Some(13),
            children: vec![10, 11],
            mesh_index: -1,
        },
        Node {
            name: "gungauge",
            parent: Some(9),
            children: vec![12],
            mesh_index: -1,
        },
        Node {
            name: "4char_ammo",
            parent: Some(9),
            children: Vec::new(),
            mesh_index: -1,
        },
        Node {
            name: "needle",
            parent: Some(10),
            children: Vec::new(),
            mesh_index: 50,
        },
        Node {
            name: "world1",
            parent: None,
            children: vec![0, 9],
            mesh_index: -1,
        },
    ];
    GameZNodes {
        nodes: specs
            .iter()
            .enumerate()
            .map(|(index, spec)| raw_node(spec, index as u32))
            .collect(),
        ..synthetic_archive()
    }
}

/// Both airframes, as `CockpitAirframe`s bound to the authored archive.
fn content_airframes() -> [CockpitAirframe; 2] {
    [
        airframe("synthetic.kestrel", "player_kestrel"),
        airframe("synthetic.warhawk", "player_warhawk"),
    ]
}

/// The content report over the authored corpus: script → bindings → coverage →
/// content, every step the production pass.
fn content_report() -> CockpitContentReport {
    let bytes = content_cockpit_script();
    let discovery =
        discover_cockpit_bindings(&decode(&bytes), &cockpit_claim(), FIXTURE, observed(0, 16))
            .expect("the authored container declares bindings");
    let archive = content_archive();
    let coverage =
        audit_cockpit_coverage(&discovery, &content_airframes(), &archive, &archive_id())
            .expect("both roots exist");
    resolve_cockpit_content(&coverage, &archive, &archive_id())
        .expect("the report and the array measure the same archive")
}

/// The coverage report behind [`content_report`], returned beside it.
fn content_coverage() -> cs_app::camera::coverage::CockpitCoverageReport {
    let bytes = content_cockpit_script();
    let discovery =
        discover_cockpit_bindings(&decode(&bytes), &cockpit_claim(), FIXTURE, observed(0, 16))
            .expect("the authored container declares bindings");
    audit_cockpit_coverage(
        &discovery,
        &content_airframes(),
        &content_archive(),
        &archive_id(),
    )
    .expect("both roots exist")
}

/// `(node_index, name, mesh_index)` triples of one subtree's drawable members.
fn geometry_triples(
    subtree: &cs_app::camera::coverage::CockpitSubtreeContent,
) -> Vec<(u32, &str, u32)> {
    subtree
        .geometry()
        .iter()
        .map(|node| (node.node_index(), node.name(), node.mesh_index()))
        .collect()
}

// ---------------------------------------------------------- the containers ---

/// A `NoMesh` container keeps its audit state and still names the drawable
/// geometry its subtree holds — the container node itself is not in that list,
/// and neither is the member that carries no mesh.
#[test]
fn accept_t540_a_nomesh_container_names_its_subtrees_drawable_geometry() {
    let content = content_report();
    let kestrel = content
        .row("airframe/synthetic.kestrel")
        .expect("the kestrel has a content row");

    let gauge = kestrel.binding("gungauge").expect("the binding has a row");
    assert_eq!(
        gauge.coverage(),
        &CockpitNodeCoverage::NoMesh { node_index: 1 },
        "the audit's verdict travels verbatim: a container whose children draw \
         is still NoMesh, because the node the script names stores no mesh"
    );
    assert!(
        gauge.has_geometry(),
        "but the binding's subtree does carry geometry, which the audit's own \
         row could not express"
    );
    assert_eq!(gauge.subtrees().len(), 1, "one candidate, one subtree");
    let subtree = &gauge.subtrees()[0];
    assert_eq!(subtree.node_index(), 1);
    assert_eq!(
        subtree.name(),
        "gungauge",
        "the candidate's stored name, not the declared spelling"
    );
    assert_eq!(
        subtree.mesh_index(),
        -1,
        "and its own stored mesh association, signed: the container has none"
    );
    assert_eq!(
        subtree.subtree_nodes(),
        4,
        "the container, two drawable children and `markers`"
    );
    assert_eq!(
        geometry_triples(subtree),
        [(5, "ggpanel", 40), (6, "ggarrow", 44)],
        "the drawable members in node-array order: the container itself and \
         the mesh-less `markers` are members of the subtree but not of the \
         geometry list"
    );

    // The same row under another case, the way every name here is matched.
    assert!(
        kestrel.binding("GUNGAUGE").is_some(),
        "a lookup matches the row the pass wrote whatever the caller's case"
    );
}

/// A `Bound` leaf's subtree is itself: one node, carrying the same mesh the
/// audit measured.
#[test]
fn accept_t540_a_bound_leafs_subtree_is_the_leaf_itself() {
    let content = content_report();
    let kestrel = content
        .row("airframe/synthetic.kestrel")
        .expect("the kestrel has a content row");

    let tail = kestrel
        .binding("taildamage")
        .expect("the binding has a row");
    assert_eq!(
        tail.coverage(),
        &CockpitNodeCoverage::Bound {
            node_index: 4,
            mesh_index: 43
        },
        "the leaf resolved directly, and the content row carries that verdict"
    );
    let subtree = &tail.subtrees()[0];
    assert_eq!(subtree.subtree_nodes(), 1, "a leaf's closure is itself");
    assert_eq!(
        geometry_triples(subtree),
        [(4, "taildamage", 43)],
        "and the geometry it names is the leaf's own node and mesh"
    );
    assert!(
        matches!(tail.coverage().mesh_index(), Some(43)),
        "the same mesh the audit resolved"
    );
}

/// An ambiguous name stays ambiguous: one subtree per candidate, each naming
/// that candidate's own geometry, in the audit's own candidate order.
#[test]
fn accept_t540_an_ambiguous_binding_names_each_candidates_own_geometry() {
    let content = content_report();
    let kestrel = content
        .row("airframe/synthetic.kestrel")
        .expect("the kestrel has a content row");

    let ammo = kestrel
        .binding("4char_ammo")
        .expect("the binding has a row");
    assert_eq!(
        ammo.coverage(),
        &CockpitNodeCoverage::Ambiguous {
            node_indices: vec![2, 8]
        },
        "two candidates stay Ambiguous however much geometry each one holds"
    );
    assert_eq!(ammo.subtrees().len(), 2, "one subtree per candidate");
    let subtrees = ammo.subtrees();
    assert_eq!(subtrees[0].node_index(), 2);
    assert_eq!(subtrees[0].mesh_index(), 42);
    assert_eq!(
        geometry_triples(&subtrees[0]),
        [(2, "4char_ammo", 42)],
        "the airframe-level candidate is a leaf that draws itself"
    );
    assert_eq!(subtrees[1].node_index(), 8);
    assert_eq!(subtrees[1].mesh_index(), 45);
    assert_eq!(
        geometry_triples(&subtrees[1]),
        [(8, "4char_ammo", 45)],
        "the candidate inside missilegauge draws a different mesh"
    );
    assert_eq!(
        ammo.geometry()
            .iter()
            .map(|node| node.node_index())
            .collect::<Vec<_>>(),
        [2, 8],
        "the binding-level union names both candidates' geometry, in index order"
    );
}

/// `Absent` has no candidates and no content, and a `NoMesh` leaf's subtree is
/// itself with nothing drawable: both are named, neither is dropped.
#[test]
fn accept_t540_absent_and_meshless_leaves_report_contentless_subtrees() {
    let content = content_report();

    let kestrel = content
        .row("airframe/synthetic.kestrel")
        .expect("the kestrel has a content row");
    let absent = kestrel.binding("absent_node").expect("the row exists");
    assert_eq!(absent.coverage(), &CockpitNodeCoverage::Absent);
    assert!(
        absent.subtrees().is_empty(),
        "no candidates, so no subtrees and no geometry"
    );
    assert!(!absent.has_geometry());

    let warhawk = content
        .row("airframe/synthetic.warhawk")
        .expect("the warhawk has a content row");
    let ammo = warhawk.binding("4char_ammo").expect("the row exists");
    assert_eq!(
        ammo.coverage(),
        &CockpitNodeCoverage::NoMesh { node_index: 11 },
        "the warhawk's 4char_ammo is a mesh-less leaf, not an ambiguous pair"
    );
    assert_eq!(ammo.subtrees()[0].subtree_nodes(), 1);
    assert!(
        ammo.subtrees()[0].geometry().is_empty(),
        "its subtree is itself, and itself carries no mesh"
    );

    // The airframe-level views name the contentless bindings in declared
    // order, so a reader sees exactly which of the twenty came back empty.
    assert_eq!(
        kestrel
            .unresolved()
            .iter()
            .map(|binding| binding.node())
            .collect::<Vec<_>>(),
        ["absent_node"],
        "the kestrel resolves every declared binding's subtree to geometry \
         except the name nothing stores"
    );
    assert!(
        !kestrel.is_complete(),
        "and the row says so: four of five resolve, one stays empty"
    );
    assert_eq!(
        warhawk
            .unresolved()
            .iter()
            .map(|binding| binding.node())
            .collect::<Vec<_>>(),
        ["4char_ammo", "missilegauge", "taildamage", "absent_node"],
        "the warhawk's gauge container resolves, and everything else is either \
         absent or drawable-free, each named rather than dropped"
    );
    assert!(!warhawk.is_complete());

    assert_eq!(content.declared_pairs(), 10);
    assert_eq!(
        content.resolved_pairs(),
        5,
        "kestrel 4 + warhawk 1: a pair resolves when its subtree draws at all"
    );
    let unresolved: Vec<(&str, &str)> = content
        .unresolved_pairs()
        .map(|(airframe, binding)| (airframe, binding.node()))
        .collect();
    assert_eq!(
        unresolved,
        [
            ("airframe/synthetic.kestrel", "absent_node"),
            ("airframe/synthetic.warhawk", "4char_ammo"),
            ("airframe/synthetic.warhawk", "missilegauge"),
            ("airframe/synthetic.warhawk", "taildamage"),
            ("airframe/synthetic.warhawk", "absent_node"),
        ],
        "report-level pairs, in row order"
    );
    assert!(!content.is_complete());
}

/// The content walk cannot leave the airframe: a candidate's closure follows
/// children only, so the parent chain through `world1` into the other
/// aircraft's subtree is never taken.
#[test]
fn accept_t540_a_bindings_content_never_leaves_its_own_airframe() {
    let content = content_report();
    let kestrel = content
        .row("airframe/synthetic.kestrel")
        .expect("the kestrel has a content row");
    let warhawk = content
        .row("airframe/synthetic.warhawk")
        .expect("the warhawk has a content row");

    let kestrel_nodes: BTreeSet<u32> = kestrel
        .drawable_nodes()
        .iter()
        .map(|node| node.node_index())
        .collect();
    let warhawk_nodes: BTreeSet<u32> = warhawk
        .drawable_nodes()
        .iter()
        .map(|node| node.node_index())
        .collect();
    assert!(
        kestrel_nodes.is_disjoint(&warhawk_nodes),
        "no drawable node is attributed to both aircraft: {kestrel_nodes:?} vs \
         {warhawk_nodes:?}"
    );
    assert_eq!(
        kestrel_nodes,
        [2, 4, 5, 6, 8].into_iter().collect(),
        "the kestrel's five drawable nodes, none of them warhawk's needle or \
         the world above the shared parent"
    );
    assert_eq!(
        warhawk_nodes,
        [12].into_iter().collect(),
        "and the warhawk's one, its own needle — never the kestrel's meshes"
    );

    // Both airframes bind a `gungauge`; each row names its own subtree.
    assert_eq!(
        geometry_triples(&kestrel.binding("gungauge").unwrap().subtrees()[0]),
        [(5, "ggpanel", 40), (6, "ggarrow", 44)],
        "the kestrel's gauge contents"
    );
    assert_eq!(
        geometry_triples(&warhawk.binding("gungauge").unwrap().subtrees()[0]),
        [(12, "needle", 50)],
        "the warhawk's — the same binding name, its own aircraft's content"
    );
    assert!(
        kestrel
            .drawable_nodes()
            .iter()
            .all(|node| node.name() != "needle" && node.mesh_index() != 50),
        "nothing of the warhawk's subtree leaks into a kestrel row, and the \
         world node above both roots is never reached through a parent edge"
    );
}

/// Two declared bindings legitimately name one node: kestrel's `4char_ammo`
/// second candidate is the single drawable member of `missilegauge`'s subtree.
/// Per-binding rows each name it; the airframe union counts it once.
#[test]
fn accept_t540_the_airframe_union_counts_a_shared_node_once() {
    let content = content_report();
    let kestrel = content
        .row("airframe/synthetic.kestrel")
        .expect("the kestrel has a content row");

    let per_binding: usize = kestrel
        .bindings()
        .iter()
        .map(|binding| binding.geometry().len())
        .sum();
    assert_eq!(
        per_binding, 6,
        "2 + 2 + 1 + 1 drawable members across the four resolving bindings, \
         with node 8 counted once under 4char_ammo and once under missilegauge"
    );
    let union: Vec<u32> = kestrel
        .drawable_nodes()
        .iter()
        .map(|node| node.node_index())
        .collect();
    assert_eq!(
        union,
        [2, 4, 5, 6, 8],
        "five distinct instruments in the kestrel's cockpit, in index order"
    );

    let missile = kestrel.binding("missilegauge").expect("the row exists");
    assert_eq!(
        missile.coverage(),
        &CockpitNodeCoverage::NoMesh { node_index: 3 },
        "missilegauge is the other NoMesh container, verbatim"
    );
    assert_eq!(missile.subtrees()[0].subtree_nodes(), 2);
    assert_eq!(
        geometry_triples(&missile.subtrees()[0]),
        [(8, "4char_ammo", 45)],
        "whose only drawable member is the ambiguous binding's second candidate"
    );
}

/// The pass reports a coverage row measured over another archive, and a node
/// index this array does not hold, each by name — never by dropping them.
#[test]
fn accept_t540_a_mismatched_or_truncated_archive_is_refused_not_walked_past() {
    let coverage = content_coverage();
    let archive = content_archive();

    // A report audited over ZBD/planes.zbd cannot resolve against another
    // container's array: the node indices it stored belong to the first.
    let other_id = ContentId::from_source(
        ContentKind::InstallFile,
        &cs_content::catalog::baseline::install_file_key("ZBD/c1/gamez.zbd"),
    )
    .expect("the other archive's key is a valid id key");
    assert_eq!(
        resolve_cockpit_content(&coverage, &archive, &other_id),
        Err(CockpitCoverageError::ContainerMismatch {
            airframe: "airframe/synthetic.kestrel".to_owned(),
            declared: archive_id().as_str().to_owned(),
            measured: other_id.as_str().to_owned(),
        }),
        "the row's own container answers which archive it was measured over"
    );

    // The same report against an array that lost node 8: the audit resolved
    // `4char_ammo` to [2, 8], and a resolver that skipped the missing index
    // would report a second subtree that silently does not exist.
    let truncated = GameZNodes {
        nodes: archive.nodes[..8].to_vec(),
        ..archive
    };
    assert_eq!(
        resolve_cockpit_content(&coverage, &truncated, &archive_id()),
        Err(CockpitCoverageError::ResolvedNodeAbsent {
            airframe: "airframe/synthetic.kestrel".to_owned(),
            node: "4char_ammo".to_owned(),
            node_index: 8,
        }),
        "an index the report resolved must be in the array it resolves against"
    );
}

/// Row for row, binding for binding, the content report carries the coverage
/// verdict it was built on — so "resolved to drawable content" can never be
/// misread as "the direct binding resolved".
#[test]
fn accept_t540_the_content_report_carries_the_audit_states_verbatim() {
    let coverage = content_coverage();
    let archive = content_archive();
    let content = resolve_cockpit_content(&coverage, &archive, &archive_id())
        .expect("the report and the array measure the same archive");

    assert_eq!(content.rows().len(), coverage.rows().len());
    assert_eq!(content.archive(), archive_id().as_str());
    for (content_row, coverage_row) in content.rows().iter().zip(coverage.rows()) {
        assert_eq!(content_row.airframe(), coverage_row.airframe());
        assert_eq!(content_row.root(), coverage_row.root());
        assert_eq!(content_row.root_index(), coverage_row.root_index());
        assert_eq!(content_row.container(), coverage_row.container());
        assert_eq!(content_row.bindings().len(), coverage_row.bindings().len());
        for (content_binding, coverage_binding) in
            content_row.bindings().iter().zip(coverage_row.bindings())
        {
            assert_eq!(content_binding.node(), coverage_binding.node());
            assert_eq!(
                content_binding.coverage(),
                coverage_binding.coverage(),
                "{}'s {}: the verdict is the audit's own, unchanged",
                content_row.airframe(),
                content_binding.node()
            );
        }
    }

    // The decoupling, stated outright on the kestrel: the audit leaves four of
    // five bindings unresolved, and the content pass resolves four of five —
    // but they are not the same four, because a NoMesh container draws its
    // children while the ambiguous pair and the absent name behave differently.
    let coverage_kestrel = coverage
        .row("airframe/synthetic.kestrel")
        .expect("the audit row exists");
    assert_eq!(
        coverage_kestrel
            .unresolved()
            .iter()
            .map(|binding| binding.node())
            .collect::<Vec<_>>(),
        ["gungauge", "4char_ammo", "missilegauge", "absent_node"],
        "the audit's unresolved set"
    );
    let content_kestrel = content
        .row("airframe/synthetic.kestrel")
        .expect("the content row exists");
    assert_eq!(
        content_kestrel
            .unresolved()
            .iter()
            .map(|binding| binding.node())
            .collect::<Vec<_>>(),
        ["absent_node"],
        "and the content pass's — the containers and the ambiguous name all \
         name drawable geometry; only the absent one stays empty"
    );
}

// ------------------------------------------------------------- the retail --

/// The measured corpus: every declared binding of every airframe resolves to
/// drawable geometry inside its own subtree, with the audit's states carried
/// verbatim — the forty-four pairs F21-D left unresolved split into the
/// twenty-two `NoMesh` gauge containers whose descendants draw and the
/// twenty-two `Ambiguous` character nodes whose two candidates each draw.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t540_retail_every_declared_binding_names_drawable_geometry_in_its_own_subtree() {
    use cs_formats::gamez::{read_gamez_meshes, read_gamez_nodes};
    use cs_formats::interp::decode_interp;
    use cs_formats::io::ParseContext;

    let game_dir = retail_dir();
    let discovery = cs_assets::install::discover(&game_dir).expect("production discovery reads it");
    let install_sha256 = cs_assets::install::fingerprint(&discovery.manifest);
    let container_label = "ZBD/interp.zbd";
    let bytes = std::fs::read(game_dir.join("ZBD").join("interp.zbd"))
        .expect("the loading-script container must be there");
    let decoded = decode_interp(&mut ParseContext::with_defaults(container_label), &bytes)
        .expect("the real container decodes");
    let bindings = discover_cockpit_bindings(
        &decoded,
        &retail_cockpit_claim(install_sha256),
        container_label,
        retail_provenance(install_sha256, container_label, 12_850, 5_713),
    )
    .expect("the claimed script is the one the container holds");

    let roster = discover_airframe_roster(
        &decoded,
        &retail_roster_declarations(install_sha256, &decoded),
    )
    .expect("the real roster discovery reports findings, not a contradiction");
    assert_eq!(roster.airframe_count(), RETAIL_AIRFRAMES);
    let airframes: Vec<CockpitAirframe> = roster
        .discovered()
        .iter()
        .map(CockpitAirframe::from)
        .collect();

    let archive_label = "ZBD/planes.zbd";
    let archive_id = ContentId::from_source(
        ContentKind::InstallFile,
        &cs_content::catalog::baseline::install_file_key(archive_label),
    )
    .expect("the normalized install-file key is a valid id key");
    let planes = std::fs::read(game_dir.join("ZBD").join("planes.zbd"))
        .expect("the shared airframe archive must be there");
    let declared_archive = discovery
        .manifest
        .files
        .iter()
        .find(|row| row.relative_spelling.logical_key() == "zbd/planes.zbd")
        .map(|row| row.sha256)
        .expect("the airframe archive is inventoried");
    assert_eq!(
        cs_assets::install::sha256(&planes),
        declared_archive,
        "the node array the content pass walks is the array the installation \
         manifest declares for zbd/planes.zbd"
    );
    let nodes = read_gamez_nodes(&mut ParseContext::with_defaults(archive_label), &planes)
        .expect("the airframe archive's node array reads");

    let coverage = audit_cockpit_coverage(&bindings, &airframes, &nodes, &archive_id)
        .expect("every declared root exists in the real archive");
    let content = resolve_cockpit_content(&coverage, &nodes, &archive_id)
        .expect("the content pass resolves over the archive the audit measured");

    assert_eq!(content.archive(), archive_id.as_str());
    assert_eq!(content.rows().len(), RETAIL_AIRFRAMES);
    assert_eq!(
        content.declared_pairs(),
        220,
        "eleven airframes of twenty declared bindings each"
    );
    // The task's headline measurement: every declared binding's own subtree
    // draws, including all forty-four pairs the coverage audit leaves
    // unresolved — so the report's `unresolved_pairs` is empty here without
    // any binding's direct state having changed.
    assert_eq!(
        content.resolved_pairs(),
        220,
        "every declared binding's subtree holds drawable geometry"
    );
    assert_eq!(content.unresolved_pairs().count(), 0);
    assert!(content.is_complete());

    // The audit's unresolved pairs are exactly the names the content pass
    // explains, and every one of them has drawable content.
    let mut coverage_unresolved = 0_usize;
    for row in coverage.rows() {
        for binding in row.unresolved() {
            coverage_unresolved += 1;
            let content_row = content.row(row.airframe()).expect("same airframes");
            assert!(
                content_row
                    .binding(binding.node())
                    .expect("same bindings")
                    .has_geometry(),
                "{}'s {} is unresolved directly but its subtree must draw",
                row.airframe(),
                binding.node()
            );
        }
    }
    assert_eq!(
        coverage_unresolved, 44,
        "the F21-D unresolved set the content pass had to cover"
    );

    // The mesh section answers whether the slots the geometry nodes name hold
    // meshes, against the same file the node array came from.
    let meshes = read_gamez_meshes(
        &mut ParseContext::with_defaults(archive_label),
        archive_label,
        &planes,
    )
    .expect("the archive's mesh section reads");

    // Node names are the stored spellings, measured by the probe over
    // planes.zbd on 2026-10-11: a `gungauge` subtree holds its container, a
    // `markers` group that stores no mesh, and eight drawable children.
    let gun_drawable: BTreeSet<&str> = [
        "4char_ammo",
        "ggindicator0",
        "ggindicator1",
        "ggindicator2",
        "ggindicator3",
        "ggarrow",
        "6char_type",
        "g815",
    ]
    .into_iter()
    .collect();
    let missile_drawable: BTreeSet<&str> = [
        "4char_ammo",
        "mgindicator0",
        "mgindicator1",
        "mgindicator2",
        "mgindicator3",
        "mgindicator4",
        "mgindicator5",
        "mgindicator6",
        "mgindicator7",
        "mgarrow",
        "6char_type",
        "g819",
    ]
    .into_iter()
    .collect();

    // Every drawable node index a row reports must stay inside that row's own
    // airframe; collecting the per-airframe sets and checking they are
    // pairwise disjoint is the check that no walk crossed a boundary.
    let mut unions: Vec<BTreeSet<u32>> = Vec::with_capacity(content.rows().len());
    let mut checked_slots = 0_usize;
    for (row, coverage_row) in content.rows().iter().zip(coverage.rows()) {
        assert_eq!(row.bindings().len(), RETAIL_COCKPIT_BINDINGS.len());
        assert_eq!(row.container(), coverage_row.container());

        for (binding, coverage_binding) in row.bindings().iter().zip(coverage_row.bindings()) {
            assert_eq!(
                binding.coverage(),
                coverage_binding.coverage(),
                "{}'s {}: verbatim",
                row.airframe(),
                binding.node()
            );
            // Every drawable node the pass names is a real slot of the real
            // mesh array, never an index past its end.
            for subtree in binding.subtrees() {
                assert!(
                    subtree.subtree_nodes() >= subtree.geometry().len(),
                    "the drawable list is a subset of the subtree"
                );
                for node in subtree.geometry() {
                    let slot = meshes
                        .meshes
                        .get(node.mesh_index() as usize)
                        .unwrap_or_else(|| {
                            panic!(
                                "{}'s {} names mesh {} inside the mesh array",
                                row.airframe(),
                                binding.node(),
                                node.mesh_index()
                            )
                        });
                    assert!(
                        slot.is_some(),
                        "{}'s {} names mesh {}, an empty stub slot",
                        row.airframe(),
                        binding.node(),
                        node.mesh_index()
                    );
                    checked_slots += 1;
                    let stored = nodes
                        .get(node.node_index())
                        .expect("the node index is in the array");
                    assert_eq!(
                        stored.info.mesh_index as u32,
                        node.mesh_index(),
                        "{}'s {}: the drawable node stores the association the \
                         report names",
                        row.airframe(),
                        binding.node()
                    );
                }
            }
        }

        // --- the four bindings whose direct state is not `Bound` ---
        let gun = row.binding("gungauge").expect("every airframe declares it");
        assert!(matches!(gun.coverage(), CockpitNodeCoverage::NoMesh { .. }));
        let gun_subtree = &gun.subtrees()[0];
        assert_eq!(
            gun_subtree.subtree_nodes(),
            10,
            "{}'s gungauge subtree",
            row.airframe()
        );
        assert_eq!(
            gun_subtree
                .geometry()
                .iter()
                .map(|node| node.name())
                .collect::<BTreeSet<_>>(),
            gun_drawable,
            "{}'s gungauge names the eight drawable instrument children",
            row.airframe()
        );
        assert_eq!(gun_subtree.geometry().len(), 8);

        let missile = row
            .binding("missilegauge")
            .expect("every airframe declares it");
        assert!(matches!(
            missile.coverage(),
            CockpitNodeCoverage::NoMesh { .. }
        ));
        let missile_subtree = &missile.subtrees()[0];
        assert_eq!(
            missile_subtree.subtree_nodes(),
            14,
            "{}'s missilegauge subtree",
            row.airframe()
        );
        assert_eq!(
            missile_subtree
                .geometry()
                .iter()
                .map(|node| node.name())
                .collect::<BTreeSet<_>>(),
            missile_drawable,
            "{}'s missilegauge names its twelve drawable children",
            row.airframe()
        );
        assert_eq!(missile_subtree.geometry().len(), 12);

        // Each ambiguous binding resolves to two candidates, one inside each
        // gauge's subtree, each a leaf drawing its own mesh.
        for name in ["4char_ammo", "6char_type"] {
            let binding = row.binding(name).expect("every airframe declares it");
            let CockpitNodeCoverage::Ambiguous { node_indices } = binding.coverage() else {
                panic!(
                    "{}'s {name} is the ambiguous binding, got {:?}",
                    row.airframe(),
                    binding.coverage()
                )
            };
            assert_eq!(node_indices.len(), 2);
            assert_eq!(binding.subtrees().len(), 2);
            let mut candidate_meshes = Vec::new();
            for subtree in binding.subtrees() {
                assert_eq!(
                    subtree.subtree_nodes(),
                    1,
                    "each {name} candidate is a leaf"
                );
                assert_eq!(subtree.name(), name);
                assert_eq!(geometry_triples(subtree).len(), 1, "and draws itself");
                candidate_meshes.push(subtree.mesh_index());
            }
            assert_ne!(
                candidate_meshes[0],
                candidate_meshes[1],
                "{}'s {name} candidates draw different meshes",
                row.airframe()
            );
            // And the two candidates are exactly the `name`-named drawable
            // members inside the two gauge subtrees — the containers' rows
            // and the ambiguous row are the same nodes seen twice.
            let inside: BTreeSet<u32> = [gun_subtree, missile_subtree]
                .into_iter()
                .flat_map(|subtree| subtree.geometry().iter())
                .filter(|node| node.name() == name)
                .map(|node| node.node_index())
                .collect();
            assert_eq!(
                inside,
                node_indices.iter().copied().collect(),
                "{}'s {name} candidates are the two gauge members, no third \
                 node anywhere",
                row.airframe()
            );
        }

        // The sixteen directly bound bindings are leaves whose subtree is
        // themselves: one node, carrying the mesh the audit resolved.
        for binding in row.bindings() {
            let CockpitNodeCoverage::Bound {
                node_index,
                mesh_index,
            } = binding.coverage()
            else {
                continue;
            };
            let subtree = &binding.subtrees()[0];
            assert_eq!(subtree.node_index(), *node_index);
            assert_eq!(subtree.subtree_nodes(), 1);
            assert_eq!(
                geometry_triples(subtree),
                [(*node_index, binding.node(), *mesh_index)],
                "{}'s {} is a leaf binding that names itself",
                row.airframe(),
                binding.node()
            );
        }

        // The per-airframe union: the two gauges' contents plus the four
        // damage nodes, with the indicator and character bindings' nodes
        // counted once however many bindings name them.
        let union = row.drawable_nodes();
        assert_eq!(
            union.len(),
            24,
            "{} names twenty-four distinct drawable cockpit nodes",
            row.airframe()
        );
        unions.push(union.iter().map(|node| node.node_index()).collect());
    }
    assert!(
        checked_slots > 220,
        "every drawable member's mesh slot was checked against the archive's \
         own mesh array, not just once per binding: {checked_slots}"
    );
    for (index, left) in unions.iter().enumerate() {
        for right in &unions[index + 1..] {
            assert!(
                left.is_disjoint(right),
                "two airframes' drawable sets must never share a node index"
            );
        }
    }
    assert_eq!(
        unions.iter().map(BTreeSet::len).sum::<usize>(),
        264,
        "eleven airframes of twenty-four drawable cockpit nodes each"
    );

    write_content_census(
        &coverage,
        &content,
        install_sha256,
        cs_assets::install::content_fingerprint(&discovery.manifest),
    );
}

// ------------------------------------------------- the retail fixtures ----

/// Writes the measured subtree census under `CS_EVIDENCE_DIR` when the
/// evidence run sets it, so the acceptance report carries this run's own
/// numbers instead of only the log lines they are printed on.
///
/// Outside an evidence run the variable is absent and nothing is written: the
/// test asserts the same measurements either way. The file carries the
/// candidate tree it was produced from so a census left over from an earlier
/// commit is refused rather than reused.
///
/// What it holds is **derived** data: counts, stored node names, node and mesh
/// indices and digests. No original display text, no stored geometry and no
/// decompiled code, and nothing the log does not already state.
fn write_content_census(
    coverage: &cs_app::camera::coverage::CockpitCoverageReport,
    content: &CockpitContentReport,
    install_sha256: ContentHash,
    content_sha256: ContentHash,
) {
    let Ok(dir) = std::env::var("CS_EVIDENCE_DIR") else {
        return;
    };
    let root = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .expect("git runs")
        .stdout;
    let root = String::from_utf8_lossy(&root).trim().to_owned();
    let directory = if Path::new(&dir).is_absolute() {
        PathBuf::from(&dir)
    } else {
        Path::new(&root).join(&dir)
    };
    std::fs::create_dir_all(&directory).expect("the evidence directory is writable");
    let tree = std::process::Command::new("git")
        .args(["rev-parse", "HEAD^{tree}"])
        .current_dir(&root)
        .output()
        .expect("git runs")
        .stdout;
    let tree = String::from_utf8_lossy(&tree).trim().to_owned();

    let airframes: Vec<String> = content
        .rows()
        .iter()
        .map(|row| {
            let bindings: Vec<String> = row
                .bindings()
                .iter()
                .map(|binding| {
                    let subtrees: Vec<String> = binding
                        .subtrees()
                        .iter()
                        .map(|subtree| {
                            let geometry: Vec<String> = subtree
                                .geometry()
                                .iter()
                                .map(|node| {
                                    format!(
                                        "{{\"node_index\":{},\"name\":{},\"mesh_index\":{}}}",
                                        node.node_index(),
                                        quote(node.name()),
                                        node.mesh_index(),
                                    )
                                })
                                .collect();
                            format!(
                                "{{\"node_index\":{},\"name\":{},\"mesh_index\":{},\
                                 \"subtree_nodes\":{},\"geometry\":[{}]}}",
                                subtree.node_index(),
                                quote(subtree.name()),
                                subtree.mesh_index(),
                                subtree.subtree_nodes(),
                                geometry.join(","),
                            )
                        })
                        .collect();
                    format!(
                        "{{\"node\":{},\"state\":{},\"subtrees\":[{}]}}",
                        quote(binding.node()),
                        quote(coverage_state(binding.coverage())),
                        subtrees.join(","),
                    )
                })
                .collect();
            let coverage_row = coverage.row(row.airframe()).expect("same airframes");
            format!(
                "{{\"airframe\":{},\"root\":{},\"root_index\":{},\"declared\":{},\"bound\":{},\
                 \"drawable_nodes\":{},\"resolved\":{},\"bindings\":[{}]}}",
                quote(row.airframe()),
                quote(row.root()),
                row.root_index(),
                row.bindings().len(),
                coverage_row.bound(),
                row.drawable_nodes().len(),
                row.resolved(),
                bindings.join(","),
            )
        })
        .collect();

    let document = format!(
        "{{\"schema\":\"cs-t540-cockpit-content/1\",\"candidate_tree\":{},\
         \"install_sha256\":{},\"content_sha256\":{},\"archive\":{},\
         \"declared_pairs\":{},\"resolved_pairs\":{},\"airframes\":[{}]}}\n",
        quote(&tree),
        quote(&install_sha256.to_hex()),
        quote(&content_sha256.to_hex()),
        quote(content.archive()),
        content.declared_pairs(),
        content.resolved_pairs(),
        airframes.join(","),
    );
    let path = directory.join("binding-subtree-census.json");
    std::fs::write(&path, &document)
        .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
    println!("wrote {}", path.display());
}
