//! Acceptance stage VS-M01-RT-HOST-BLOCK-LIFECYCLES (Rally #1282): the
//! production reader that turns a control record's `BEGIN_DORMANT` and
//! `TICK_DEPENDS_ON_OBJ` spellings into the [`BlockLifecycleTable`] the
//! mission host folds, instead of the empty fail-closed table every stage
//! used to carry.
//!
//! [`cs_app::control_lifecycles::read_block_lifecycles`] walks a decoded
//! control document with the record's own asymmetric directive grammar and
//! declares each numbered block's starting lifecycle; the stage carries the
//! complete read on its seed
//! ([`cs_app::mission_session::MissionHostSeed::block_lifecycles`]) and the
//! host folds it into the fact `Condition::ObjectiveAwake` answers from.
//! Four things are held to the stage:
//!
//! * **The measured spellings read back their measured states** (CI member):
//!   a document in the installation's measured shapes — a block with no
//!   `BEGIN_DORMANT` reads awake, a `-1` sentinel block reads dormant, a
//!   positive child0 arms the timed self-wake, and a `TICK_DEPENDS_ON_OBJ`
//!   target gates the dependent's wake exactly as the measured pass-1 gate
//!   does.
//! * **A spelling outside the measured shapes is refused by block and key**
//!   (CI member), never guessed into a wake time — and the mission host
//!   keeps the empty fail-closed table for an incomplete read (the seed's
//!   consumer rule, exercised here over the same reader's refusals).
//! * **M01's real record declares its 58 measured blocks** (retail member,
//!   `#[ignore = "requires CS_GAME_DIR"]`): the six blocks the record
//!   leaves awake (zero-based indices 1, 2, 5, 6, 35, 54 — `OBJECTIVE2`,
//!   `3`, `6`, `7`, `36`, `55`, which spell no `BEGIN_DORMANT`) read
//!   awake, every other block reads dormant — 51 of them behind the `-1`
//!   sentinel that never self-wakes and `OBJECTIVE1` behind its positive
//!   2-second timed self-wake — and both `Finish` blocks read dormant.
//! * **The composed entry stops reporting the missing lifecycles**
//!   (retail member): M01's stage carries the declared table, so neither
//!   the stage nor [`cs_app::mission_session::MissionHost::launch`] raises
//!   [`cs_app::mission_session::MissionHostRefusal::BlockLifecycles`], and
//!   the host's folded table is the record's own.
//!
//! What is **not** claimed: no original executable ran, nothing here is
//! `verified_original`, and the table's timed self-wake advance
//! ([`BlockLifecycleTable::tick`]) is not wired into the composed step yet —
//! no measured mapping from a composed run to the original's mission clock
//! seconds exists, and M01 arms exactly one positive clock (`OBJECTIVE1` at
//! 2 s) whose wake would need that mapping to mean anything. The
//! wake/complete writers (`WAKE_OBJECTIVE*` effects, the completion
//! pipeline) belong to VS-M01-RT-MISSION-HOST's later stages; this task only
//! declares and folds the record's own starting lifecycles.

use cs_app::control_lifecycles::{BlockLifecycleRead, read_block_lifecycles};
use cs_app::mission_launch::{MissionLaunchPlan, plan_mission_launch};
use cs_app::mission_session::{
    MissionHost, MissionHostRefusal, MissionHostReport, MissionHostTick, build_headless, stage_for,
    teardown,
};
use cs_content::stunts::ZrdValue;
use cs_script::runtime::ObjectiveLifecycle;
use cs_sim::mission::BlockLifecycleTable;

use crate::common::label;
use crate::vs_m01_rt_window::game_dir;

/// The measured directive keys this reader declares from.
const BEGIN_DORMANT: &str = "BEGIN_DORMANT";
const TICK_DEPENDS_ON_OBJ: &str = "TICK_DEPENDS_ON_OBJ";

/// One measured record field, as the flat alternating record spells it: a
/// key node followed by its value node (siblings, never a nested pair).
fn field(key: &str, value: ZrdValue) -> Vec<ZrdValue> {
    vec![ZrdValue::Text(key.to_owned()), value]
}

/// A directive site whose follower is an argument list — the measured
/// asymmetric grammar's list spelling.
fn site(key: &str, args: Vec<ZrdValue>) -> Vec<ZrdValue> {
    vec![ZrdValue::Text(key.to_owned()), ZrdValue::List(args)]
}

/// One numbered block's directive list, as the record spells it.
fn block(directives: Vec<ZrdValue>) -> ZrdValue {
    ZrdValue::List(directives)
}

/// A whole control document in the measured wrapper shape: one child, the
/// flat alternating record.
fn document(fields: Vec<ZrdValue>) -> ZrdValue {
    ZrdValue::List(vec![ZrdValue::List(fields)])
}

/// **The measured lifecycle spellings declare an awake block and a dormant
/// block that read back their measured states, and the table drives the
/// timed self-wake and its dependency gate exactly as measured.**
///
/// The document is authored in the installation's measured shapes
/// (`docs/findings/2026-10-06-m01-lc-directive-b-objective-lifecycle-target-
/// semantics.md`): no `BEGIN_DORMANT` leaves a block awake, the key with a
/// `-1` child0 starts dormant and never self-wakes, a positive child0 arms
/// the wake at that mission-clock second, and `TICK_DEPENDS_ON_OBJ`'s
/// `child0 - 1` gates the dependent's whole lifecycle while the dependency
/// is not awake. This member runs in CI.
#[test]
fn accept_vs_m01_rt_host_block_lifecycles_measured_spellings_read_back_their_measured_states() {
    let record = document(
        [
            // A record-level key that is not a block: never read as one.
            field(
                "SOUND",
                ZrdValue::List(vec![ZrdValue::Text("music".to_owned())]),
            ),
            // Block 0: dormant, `-1` sentinel — never self-wakes.
            field(
                "OBJECTIVE1",
                block(site(BEGIN_DORMANT, vec![ZrdValue::Float(-1.0)])),
            ),
            // Block 1: no `BEGIN_DORMANT` — the record's defaults leave it
            // awake.
            field("OBJECTIVE2", block(Vec::new())),
            // Block 2: dormant with a positive wake second, tick-dependent on
            // block 1 (spelled one-based: 2 → index 1).
            field(
                "OBJECTIVE3",
                block(vec![
                    ZrdValue::Text(BEGIN_DORMANT.to_owned()),
                    ZrdValue::List(vec![ZrdValue::Float(10.5)]),
                    ZrdValue::Text(TICK_DEPENDS_ON_OBJ.to_owned()),
                    ZrdValue::List(vec![ZrdValue::Int(2)]),
                ]),
            ),
            // Block 3: dormant with a wake second already reached, but
            // tick-dependent on block 0 — the `-1` sentinel block, which is
            // never awake — so the measured pass-1 gate freezes it.
            field(
                "OBJECTIVE4",
                block(vec![
                    ZrdValue::Text(BEGIN_DORMANT.to_owned()),
                    ZrdValue::List(vec![ZrdValue::Float(3.0)]),
                    ZrdValue::Text(TICK_DEPENDS_ON_OBJ.to_owned()),
                    ZrdValue::List(vec![ZrdValue::Int(1)]),
                ]),
            ),
        ]
        .concat(),
    );

    let read = read_block_lifecycles(&record);
    assert!(
        read.is_complete(),
        "every spelling here is inside the measured shapes: {:?}",
        read.refusals()
    );
    let table = read.table();
    assert_eq!(table.len(), 4, "every numbered block declares a lifecycle");
    assert_eq!(
        table.state(0),
        Some(ObjectiveLifecycle::Dormant),
        "the `-1` sentinel block must start dormant"
    );
    assert_eq!(
        table.state(1),
        Some(ObjectiveLifecycle::Awake),
        "a block with no BEGIN_DORMANT must start awake"
    );
    assert_eq!(
        table.state(2),
        Some(ObjectiveLifecycle::Dormant),
        "a positive wake second still starts the block dormant"
    );
    assert_eq!(
        table.state(3),
        Some(ObjectiveLifecycle::Dormant),
        "a positive wake second still starts the gated block dormant"
    );

    // The facts the fold publishes are the declared states, keyed by the
    // zero-based record index the condition gate reads.
    let facts = table.facts();
    assert_eq!(facts.objectives.get(&0), Some(&ObjectiveLifecycle::Dormant));
    assert_eq!(facts.objectives.get(&1), Some(&ObjectiveLifecycle::Awake));
    assert_eq!(facts.objectives.get(&2), Some(&ObjectiveLifecycle::Dormant));
    assert_eq!(facts.objectives.get(&3), Some(&ObjectiveLifecycle::Dormant));

    // The measured wake advance: the `-1` sentinel never wakes, and no
    // block wakes before its spelled clock second; once the clock reaches
    // it with the dependency awake, the dependent wakes — while a dependent
    // whose dependency is not awake stays frozen however far the clock
    // runs (the measured pass-1 gate over the whole lifecycle).
    let mut table = table.clone();
    assert!(
        table.tick(5.0).is_empty(),
        "before the spelled second, no block wakes: the sentinel never arms and the dependent's \
         clock has not been reached"
    );
    assert_eq!(
        table.state(0),
        Some(ObjectiveLifecycle::Dormant),
        "a negative wake time never arms"
    );
    assert_eq!(
        table.tick(11.0),
        vec![2],
        "with block 1 awake and the clock past 10.5, exactly the ungated dependent wakes"
    );
    assert_eq!(table.state(2), Some(ObjectiveLifecycle::Awake));
    assert_eq!(
        table.state(3),
        Some(ObjectiveLifecycle::Dormant),
        "the gated block's clock was reached but its dependency is not awake, so it stays \
         dormant"
    );
}

/// **A spelling outside the measured shapes is refused by block and key —
/// never guessed into a wake time or a dependency — and an incomplete read
/// hands no table to a host.**
///
/// The refusals quote the refusing site; the mission host's seed rule (the
/// consumer this table travels to) keeps the empty fail-closed table for an
/// incomplete read and names the refusals, which is what
/// [`BlockLifecycleRead::refusal_detail`] carries verbatim. This member
/// runs in CI.
#[test]
fn accept_vs_m01_rt_host_block_lifecycles_spellings_outside_the_measured_shapes_are_refused() {
    // Every measured-shape violation this reader names, one document each:
    // (block key, refusing directive key, the document's flat record
    // fields).
    let cases: &[(&str, &str, Vec<ZrdValue>)] = &[
        (
            "OBJECTIVE1",
            BEGIN_DORMANT,
            [field(
                "OBJECTIVE1",
                block(vec![ZrdValue::Text(BEGIN_DORMANT.to_owned())]),
            )]
            .concat(),
        ),
        (
            "OBJECTIVE1",
            BEGIN_DORMANT,
            [field(
                "OBJECTIVE1",
                block(site(
                    BEGIN_DORMANT,
                    vec![ZrdValue::Float(-1.0), ZrdValue::Float(3.0)],
                )),
            )]
            .concat(),
        ),
        (
            "OBJECTIVE1",
            BEGIN_DORMANT,
            [field(
                "OBJECTIVE1",
                block(site(BEGIN_DORMANT, vec![ZrdValue::Text("soon".to_owned())])),
            )]
            .concat(),
        ),
        (
            "OBJECTIVE1",
            BEGIN_DORMANT,
            [field(
                "OBJECTIVE1",
                block(vec![
                    ZrdValue::Text(BEGIN_DORMANT.to_owned()),
                    ZrdValue::Int(3),
                ]),
            )]
            .concat(),
        ),
        (
            "OBJECTIVE1",
            TICK_DEPENDS_ON_OBJ,
            [field(
                "OBJECTIVE1",
                block(vec![ZrdValue::Text(TICK_DEPENDS_ON_OBJ.to_owned())]),
            )]
            .concat(),
        ),
        (
            "OBJECTIVE1",
            TICK_DEPENDS_ON_OBJ,
            [field(
                "OBJECTIVE1",
                block(site(
                    TICK_DEPENDS_ON_OBJ,
                    vec![ZrdValue::Text("block".to_owned())],
                )),
            )]
            .concat(),
        ),
        (
            "OBJECTIVE1",
            TICK_DEPENDS_ON_OBJ,
            [field(
                "OBJECTIVE1",
                block(site(
                    TICK_DEPENDS_ON_OBJ,
                    vec![ZrdValue::List(vec![ZrdValue::Int(2)])],
                )),
            )]
            .concat(),
        ),
        (
            "OBJECTIVE1",
            "<block>",
            [field("OBJECTIVE1", ZrdValue::Int(5))].concat(),
        ),
    ];
    for (block_key, refusing_key, fields) in cases {
        let read = read_block_lifecycles(&document(fields.clone()));
        assert!(
            !read.is_complete(),
            "{block_key} `{refusing_key}`: this spelling must refuse, not be read"
        );
        assert!(
            read.refusals().iter().any(|refusal| {
                refusal.to_string().contains(block_key)
                    && refusal.to_string().contains(refusing_key)
            }),
            "{block_key} `{refusing_key}`: the refusal must name the block and the key, got {:?}",
            read.refusals()
        );
        let detail = read.refusal_detail();
        assert!(
            detail.contains(block_key) && detail.contains(refusing_key),
            "the host-quoted detail must carry the refusing block and key, got: {detail}"
        );
    }

    // A duplicate spelling of one lifecycle key in one block is the parse's
    // own unconditional overwrite: the last site wins, measured, not an
    // error. And a duplicate `OBJECTIVE<N>` field refuses by block — the
    // table declares one lifecycle per block index.
    let last_wins = read_block_lifecycles(&document(
        [field(
            "OBJECTIVE1",
            block(vec![
                ZrdValue::Text(BEGIN_DORMANT.to_owned()),
                ZrdValue::List(vec![ZrdValue::Float(-1.0)]),
                ZrdValue::Text(BEGIN_DORMANT.to_owned()),
                ZrdValue::List(vec![ZrdValue::Float(7.0)]),
            ]),
        )]
        .concat(),
    ));
    assert!(
        last_wins.is_complete(),
        "a repeated BEGIN_DORMANT site is the measured overwrite, not a refusal: {:?}",
        last_wins.refusals()
    );
    assert_eq!(
        last_wins.table().state(0),
        Some(ObjectiveLifecycle::Dormant),
        "the repeated site keeps the block dormant"
    );

    let duplicated = read_block_lifecycles(&document(
        [
            field(
                "OBJECTIVE1",
                block(site(BEGIN_DORMANT, vec![ZrdValue::Float(-1.0)])),
            ),
            field(
                "OBJECTIVE1",
                block(site(BEGIN_DORMANT, vec![ZrdValue::Float(4.0)])),
            ),
        ]
        .concat(),
    ));
    assert!(
        duplicated.is_complete(),
        "a repeated block key is walked positionally, exactly as the control lowering walks it: \
         {:?}",
        duplicated.refusals()
    );
    assert_eq!(
        duplicated.table().len(),
        2,
        "two block fields are two blocks by record position — the key number is the label the \
         record spells, and the lifecycle index is the position, the same key space \
         Condition::ObjectiveAwake reads"
    );
    assert_eq!(
        duplicated.table().state(1),
        Some(ObjectiveLifecycle::Dormant),
        "the second positional block keeps its own declared lifecycle"
    );
}

/// **Against the owner's installation, M01's record declares its 58 measured
/// blocks: the six the record leaves awake read awake, every other block
/// reads dormant — 51 behind the `-1` sentinel and `OBJECTIVE1` behind its
/// positive 2-second clock — with both `Finish` blocks dormant.**
///
/// The reader entry is the production one the stage calls
/// ([`read_mission_block_lifecycles`](cs_app::control_lifecycles::read_mission_block_lifecycles)),
/// over the same control member the census measures.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_vs_m01_rt_host_block_lifecycles_retail_m01_declares_its_58_measured_blocks() {
    let root = game_dir();
    let read = cs_app::control_lifecycles::read_mission_block_lifecycles(&root, "zbd/c1c/m01")
        .expect("M01's control member reads through the production control-member rule");
    assert!(
        read.is_complete(),
        "M01's lifecycle spellings are all inside the measured shapes: {:?}",
        read.refusals()
    );
    let table = read.table();
    assert_eq!(
        table.len(),
        58,
        "M01's program has 58 numbered blocks, each declaring one lifecycle"
    );
    // Measured over the owner's installation (this member re-derives it
    // through the production reader): the six blocks with no
    // `BEGIN_DORMANT` are record indices 1, 2, 5, 6, 35 and 54 —
    // `OBJECTIVE2`, `3`, `6`, `7`, `36`, `55`, the labels the record
    // spells beside them — and every other block starts dormant.
    //
    // Index is the record position, the same key space
    // `Condition::ObjectiveAwake` reads and `TICK_DEPENDS_ON_OBJ`'s
    // `child0 - 1` names, so these are not the one-based labels: the
    // record's first block field is `OBJECTIVE1`, at index 0.
    let awake: Vec<u32> = (0..58)
        .filter(|index| table.state(*index) == Some(ObjectiveLifecycle::Awake))
        .collect();
    assert_eq!(
        awake,
        vec![1, 2, 5, 6, 35, 54],
        "exactly M01's measured awake blocks must read awake"
    );
    for index in 0..58u32 {
        if awake.contains(&index) {
            continue;
        }
        assert_eq!(
            table.state(index),
            Some(ObjectiveLifecycle::Dormant),
            "block {index} must start dormant"
        );
    }
    // `Finish(Succeeded)` is block 23 and `Finish(Failed)` is block 39
    // (one-based record numbers), both dormant behind the sentinel — the
    // measured reason M01 cannot reach a terminal state in a run that
    // wakes nothing.
    assert_eq!(
        table.state(22),
        Some(ObjectiveLifecycle::Dormant),
        "Finish(Succeeded) must start dormant"
    );
    assert_eq!(
        table.state(38),
        Some(ObjectiveLifecycle::Dormant),
        "Finish(Failed) must start dormant"
    );
    // The one positive clock M01 arms: `OBJECTIVE1` (index 0) spells
    // `BEGIN_DORMANT 2`, so the record's own wake time is kept as spelled
    // and it is the only block the timed advance can ever move. Every
    // other dormant block is the `-1` sentinel, which never arms at any
    // clock. The advance is exactly `BlockLifecycleTable::tick`.
    let mut advancing = table.clone();
    assert!(
        advancing.tick(1.9).is_empty(),
        "no block wakes before the one armed clock second"
    );
    assert_eq!(
        advancing.tick(2.0),
        vec![0],
        "M01 arms exactly one timed self-wake, OBJECTIVE1 at 2 seconds, and no other block \
         self-wakes at any clock"
    );
    assert_eq!(
        advancing.tick(f64::INFINITY),
        Vec::<u32>::new(),
        "once awake the block never wakes twice; the -1 sentinels never arm"
    );
}

/// **Against the owner's installation, M01's composed stage carries the
/// declared table: the seed holds the record's 58 lifecycles, the composed
/// entry steps over them, and no `BlockLifecycles` refusal is raised.**
///
/// This is the composed entry's own member for the stage
/// (VS-M01-RT-MISSION-HOST.01's retail member holds the same seam): the
/// host folds the table the reader declared, not an empty one.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_vs_m01_rt_host_block_lifecycles_retail_m01_stage_carries_the_declared_table_to_the_host()
{
    let root = game_dir();
    let plan: MissionLaunchPlan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads through the production readers");
    drop(plan);

    assert_eq!(
        stage.host.block_lifecycles.len(),
        58,
        "the seed must carry M01's own declared block lifecycles"
    );
    assert!(
        stage
            .host
            .refusals
            .iter()
            .all(|refusal| !matches!(refusal, MissionHostRefusal::BlockLifecycles { .. })),
        "a stage that carries a declared table raises no undeclared-lifecycles refusal"
    );

    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();
    app.update();
    let report = app
        .world()
        .get_resource::<MissionHostReport>()
        .expect("the composed entry wrote its report");
    let produced: &MissionHostTick = report.answer.as_ref().unwrap_or_else(|error| {
        panic!("M01's composed step refused: {error}");
    });
    assert_eq!(produced.script.tick, report.tick);

    let host = app.world().resource::<MissionHost>();
    assert!(
        host.refusals()
            .iter()
            .all(|refusal| !matches!(refusal, MissionHostRefusal::BlockLifecycles { .. })),
        "the host must not raise the undeclared-lifecycles refusal for a stage that carries a \
         declared table"
    );
    assert_eq!(
        host.blocks().len(),
        58,
        "the host folds the record's own table, not an empty one"
    );
    // Measured over the owner's installation: `OBJECTIVE1` (index 0)
    // spells `BEGIN_DORMANT 2` and starts dormant behind its positive
    // clock; `OBJECTIVE2` (index 1) spells no `BEGIN_DORMANT` and starts
    // awake — the two shapes a declared lifecycle can take, both read
    // back through the folded table.
    assert_eq!(
        host.blocks().state(0),
        Some(ObjectiveLifecycle::Dormant),
        "M01's first block starts dormant behind its positive 2-second clock, so the folded \
         table must read it dormant"
    );
    assert_eq!(
        host.blocks().state(1),
        Some(ObjectiveLifecycle::Awake),
        "M01's second block spells no BEGIN_DORMANT, so the folded table must read it awake"
    );
    assert_eq!(
        host.blocks().state(22),
        Some(ObjectiveLifecycle::Dormant),
        "M01's Finish(Succeeded) block reads dormant through the folded table"
    );
    // The folded table is the very one the fact fold answers through: the
    // fail-closed empty-table read would publish no objective at all.
    assert_eq!(
        host.blocks().facts().objectives.len(),
        58,
        "every declared block publishes its lifecycle to the fold"
    );

    teardown(&mut app);
}

/// The reader's public surface stays callable from a test that imports it
/// through the crate root only — a compile-time seam, so a re-export rename
/// fails here rather than in a retail member.
#[test]
fn accept_vs_m01_rt_host_block_lifecycles_empty_document_declares_nothing() {
    let read: BlockLifecycleRead = read_block_lifecycles(&document(Vec::new()));
    assert!(read.is_complete(), "an empty record refuses nothing");
    assert!(read.table().is_empty());
    let mut table = BlockLifecycleTable::new();
    table
        .declare(0, cs_sim::mission::LifecycleDecl::awake())
        .expect("the first declaration succeeds");
    assert!(!table.is_empty(), "the declared table is not empty");
}
