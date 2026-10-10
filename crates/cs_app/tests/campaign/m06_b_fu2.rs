//! Acceptance follow-up **M06-B-FU2** (Rally #818): where — if anywhere —
//! M06's passenger/extraction entity lives (`missions/M06.md`, work order
//! `M06-B`, follow-up of #274).
//!
//! M06-B measured that the mission's own reader archive spells exactly one
//! passenger-named string — the location node `Passenger_hangar` in
//! `location.zrd` — and that no directive, target or actor names a passenger,
//! so the sheet's *passenger identity* priority had no actor or program to
//! predicate. This stage closes the question that left open, by measuring the
//! rest of the installation instead of inferring a mechanic from the
//! discovery cue "airborne extraction" (which is a research label, not a
//! verified id):
//!
//! * `location.zrd` is the mission's **teleport-destination list**, not an
//!   actor list: it is byte-identical to the sibling mission's copy, its four
//!   entries are a name plus two float triples (a position and a heading), and
//!   the owner's decrypted image spells the member's consumer as a Teleport
//!   feature (`Teleport`, `Current Location`, `location.bak`, and the
//!   developer annotation that the file "stores teleport data for this
//!   mission");
//! * the installation's passenger vocabulary that does exist is **shared and
//!   non-mission**: the library member `passengers.zrd` declares seventeen
//!   `ON_CALL` crew animations that drive the world node `apassengers` (M01's
//!   startup fires one of them, `call_add_jack`; M06 fires none), the chapter
//!   world `ZBD/C2/gamez.zbd` carries exactly one `apassengers` and one
//!   `passall` node that nothing in M06 addresses, and the instant-action
//!   record `ia.zrd` names `passenger_zeppelin` as one of its target classes;
//! * the only objective string id carrying the word is
//!   `MSG_OBJ_PASSENGERHANGER`, and of the census's mission-scoped archives
//!   exactly one carries it — `ZBD/C1/IA1/zrdr.zbd`, chapter one's instant
//!   action — never M06's.
//!
//! The retail members read `$CS_GAME_DIR` (and one reads
//! `$CS_ENGINE_IMAGE`), so CI skips them and the implementer and reviewer run
//! them with `--include-ignored`. The synthetic member carries the location
//! record's shape into CI without original data. Nothing here invents a
//! passenger actor, a predicate or a mechanic: the answer this stage pins is
//! that the shipped data binds none for M06.
//!
//! `missions/bindings/M06.json` is deliberately **unchanged**: its `unknowns`
//! are derived by production code from one global list every mission shares,
//! and `accept_m06_a_the_committed_record_is_what_the_installation_derives`
//! byte-compares the committed file with that derivation. Carrying this
//! verdict inside the record therefore needs a mission-scoped unknown in
//! `cs_content::campaign_bindings`, which is outside this task's owner paths —
//! it is filed as **M06-B-FU4** (#1184). Until it lands, the limitation lives
//! in `docs/findings/2026-10-10-m06-b-fu2-passenger-identity-binding.md` and
//! in the pins below.

use std::path::PathBuf;

use cs_app::mission_control::survey_mission_control_programs;
use cs_content::coordinates::{load_engine_image, original_image_digest};
use cs_content::stunts::{ZrdValue, decode_zrd, zrd_flat_fields};
use cs_formats::gamez::read_gamez_nodes;
use cs_formats::io::ParseContext;
use cs_formats::script_raw::discover_container;
use cs_types::install::RelativePath;

/// M06's reader archive, the program span `missions/bindings/M06.json` cites.
const CONTAINER: &str = "ZBD/C2/M01/zrdr.zbd";
/// SHA-256 of that whole archive, from production discovery (M06-A/M06-B).
const CONTAINER_SHA256: &str = "d6e9315d580a0570ae8744d2c1b154c4c1af9086bf45cc2fcc125f0921fd9f63";
/// The chapter world container the mission is flown in.
const WORLD_CONTAINER: &str = "ZBD/C2/gamez.zbd";
/// SHA-256 of the whole world container.
const WORLD_SHA256: &str = "2b2cf09bcaef9024937145cfdf8906446a301ec53409bf3368a965a6f4c924b6";
/// The installation-wide library archive that declares the crew animations.
const COMMON_CONTAINER: &str = "ZBD/zrdr.zbd";
/// SHA-256 of the whole library archive.
const COMMON_SHA256: &str = "76b510d821edd2268040d2ccb18c462ec07ad580cdba571b3066228e2cf592dd";
/// The library member that declares the crew animations.
const PASSENGERS_MEMBER: &str = "passengers.zrd";
/// SHA-256 of that member's own bytes.
const PASSENGERS_MEMBER_SHA256: &str =
    "2dedd152f019716e4815e14eb4ba2d64589b269041ea3974c9815f9f28ab092d";
/// The chapter's instant-action archive, whose record names the zeppelin
/// classes.
const INSTANT_ACTION_CONTAINER: &str = "ZBD/C2/IA1/zrdr.zbd";
/// SHA-256 of M06's own `location.zrd` member.
const LOCATION_MEMBER_SHA256: &str =
    "ece361bcc757158077c547630d8d96f53b55c78cd1808885115ac94630c8b724";

/// The four locations M06's `location.zrd` spells, in member order: the name,
/// its world position and its heading. Every value is record data.
const LOCATIONS: [(&str, [f32; 3], [f32; 3]); 4] = [
    (
        "Airport_terminal",
        [-5249.0, 199.0, -4747.0],
        [-11.0, 22.0, 0.0],
    ),
    (
        "Passenger_hangar",
        [-5177.0, 325.0, -6842.0],
        [-15.0, 9.0, 0.0],
    ),
    ("Crops", [-3295.0, 390.0, -6837.0], [-12.8, 151.0, 0.0]),
    ("Coast", [-1968.0, 128.0, -2465.0], [9.0, -23.0, 0.0]),
];

/// The seventeen animation names the library member declares for the crew
/// node, in declaration order. Every one is `ON_CALL`, so a mission fires the
/// crew it wants; the animations are never self-starting.
const CREW_ANIMATIONS: [&str; 17] = [
    "add_waldo",
    "add_spks",
    "add_jack",
    "call_add_jack",
    "add_bjon",
    "call_add_bjon",
    "add_pick",
    "call_add_pick",
    "add_fas",
    "call_add_fas",
    "add_ilsa",
    "call_add_ilsa",
    "add_boothe",
    "call_add_boothe",
    "add_swan",
    "call_add_swan",
    "rem_pas",
];

/// The object node the seventeen crew animations drive.
const CREW_NODE: &str = "apassengers";

/// The world nodes M06's own directives, targets and evaluator member lists
/// name, each of which must resolve in the chapter world exactly once — the
/// contrast that shows the mission addresses what it uses.
const MISSION_NODES: [&str; 14] = [
    "sprucegoose",
    "propane",
    "kkgate",
    "tugandbarge01",
    "tugandbarge02",
    "tugandbarge03",
    "tugandbarge04",
    "g_engine1",
    "g_engine2",
    "g_engine3",
    "g_engine4",
    "g_engine5",
    "g_engine6",
    "g_engine7",
    "g_engine8",
];

/// The chapter node the *sibling* missions name (`ZBD/C2/M02`,
/// `ZBD/C2/M03` and the chapter's instant-action record spell `sghangar`) and
/// M06's archive spells nowhere — the counter-example that keeps the list
/// above honest about whose names it lists.
const SIBLING_ONLY_NODE: &str = "sghangar";

/// The one message id the installation spells with the word, and the one
/// archive of the census that carries it (chapter one's instant action).
const PASSENGER_MESSAGE_ID: &str = "MSG_OBJ_PASSENGERHANGER";
const PASSENGER_MESSAGE_CARRIER: &str = "ZBD/C1/IA1/zrdr.zbd";

/// The startup animation names M06's own startup member fires, in order —
/// four at `NEW_GAME_START` and `player_setup` again at `LOAD_GAME_START`.
const FIRED_STARTUP_ANIMATIONS: [&str; 5] = [
    "player_setup",
    "kktorch_burning1",
    "kktorch_burning2",
    "place_the_goose",
    "player_setup",
];

/// The message ids M06's archive references anywhere in its fifteen members.
const MISSION_MESSAGE_IDS: [&str; 15] = [
    "MSG_BJOHN_NAME",
    "MSG_BRF_HWM2_OBJ1",
    "MSG_BRF_HWM2_OBJ2",
    "MSG_BRF_HWM2_OBJ3",
    "MSG_BUCK_NAME",
    "MSG_CSTEELE_NAME",
    "MSG_JACK_NAME",
    "MSG_OBJ_DEFEND",
    "MSG_OBJ_DESTROY",
    "MSG_PLAYER_NAME",
    "MSG_TEX_NAME",
    "MSG_TRGT_BARGE",
    "MSG_TRGT_PROPANE_TANKS",
    "MSG_TRGT_SGOOSE",
    "MSG_TRGT_SGOOSE_ENGINE",
];

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M06-B-FU2 needs the retail capability; run this suite with \
             `--include-ignored` and CS_GAME_DIR pointing at the read-only installation"
        )
    }))
}

/// One reader archive's declared members as production discovers them:
/// `(member name, whole member bytes)`, in member-table order.
fn members_of(container: &str) -> Vec<(String, Vec<u8>)> {
    let bytes = std::fs::read(game_dir().join(container))
        .unwrap_or_else(|error| panic!("{container} reads: {error}"));
    let relative = RelativePath::new(&container.to_lowercase())
        .unwrap_or_else(|error| panic!("{container} is a relative path: {error}"));
    let discovery = discover_container(&relative.logical_key(), &relative, &bytes);
    discovery
        .programs()
        .iter()
        .map(|program| {
            let member = program
                .locator()
                .member()
                .unwrap_or_else(|| panic!("{container} has a member-less program"))
                .to_owned();
            (member, program.bytes().to_vec())
        })
        .collect()
}

/// One member's bytes, by its authored name.
fn member_bytes(container: &str, member: &str) -> Vec<u8> {
    members_of(container)
        .into_iter()
        .find(|(name, _)| name == member)
        .map(|(_, bytes)| bytes)
        .unwrap_or_else(|| panic!("{container} declares {member}"))
}

/// Every `.zrd` text node of one member, sorted and de-duplicated — names
/// only, never bytes.
fn member_texts(container: &str, member: &str) -> Vec<String> {
    let document = member_document(container, member);
    let mut texts = Vec::new();
    walk_texts(&document, &mut texts);
    texts.sort();
    texts.dedup();
    texts
}

/// The `.zrd` document of one member, decoded.
fn member_document(container: &str, member: &str) -> ZrdValue {
    decode_zrd(&member_bytes(container, member))
        .unwrap_or_else(|error| panic!("{container}:{member} decodes: {error}"))
}

/// Every `.zrd` text node of a document, depth first.
fn walk_texts(value: &ZrdValue, texts: &mut Vec<String>) {
    match value {
        ZrdValue::Text(text) => texts.push(text.clone()),
        ZrdValue::List(children) => {
            for child in children {
                walk_texts(child, texts);
            }
        }
        _ => {}
    }
}

/// The `(name, position, heading)` entries of a location document, read
/// through the same walk the synthetic member pins: a one-element wrapper
/// around a flat name/value list, where each value is two float triples.
fn locations_of(document: &ZrdValue) -> Vec<(String, [f32; 3], [f32; 3])> {
    let outer = document.as_list().expect("the location document is a list");
    let inner = outer
        .first()
        .expect("the location document is the measured one-element wrapper");
    let triple = |value: &ZrdValue, name: &str| -> [f32; 3] {
        let floats: Vec<f32> = value
            .as_list()
            .unwrap_or_else(|| panic!("{name} is followed by a list"))
            .iter()
            .map(|item| match item {
                ZrdValue::Float(float) => *float,
                other => panic!("{name} spells a float, not {other:?}"),
            })
            .collect();
        <[f32; 3]>::try_from(floats.as_slice())
            .unwrap_or_else(|_| panic!("{name} spells exactly three floats"))
    };
    zrd_flat_fields(inner)
        .into_iter()
        .map(|(name, value)| {
            let parts = value
                .as_list()
                .unwrap_or_else(|| panic!("{name} is followed by a list"));
            assert_eq!(
                parts.len(),
                2,
                "{name} spells a position and a heading, nothing else"
            );
            (
                name.to_owned(),
                triple(&parts[0], name),
                triple(&parts[1], name),
            )
        })
        .collect()
}

/// The first offset of `needle` in `haystack`, if any.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

// ---------------------------------------------------------------------------
// Retail: what M06's own archive carries
// ---------------------------------------------------------------------------

/// **The only passenger-named string in M06's own data is a map location.**
///
/// The archive's fifteen members all decode; walking every member's text nodes
/// finds exactly one passenger-named string, `Passenger_hangar`, and it is in
/// `location.zrd` alone. That member is not mission-authored content: its bytes
/// are **identical** to the sibling mission's copy of the same member
/// (`ZBD/C2/M02/zrdr.zbd` carries the same four names at the same bytes), so it
/// is the chapter's shared place list, and its four entries are a name plus a
/// position and a heading — a marked point, not an actor: no kind, no plane, no
/// net, no spawn.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m06_b_fu2_the_only_passenger_string_in_m06s_own_data_is_a_map_location() {
    let bytes = std::fs::read(game_dir().join(CONTAINER)).expect("M06's reader archive reads");
    assert_eq!(
        cs_assets::install::sha256(&bytes).to_hex(),
        CONTAINER_SHA256,
        "the reader archive is the program M06-A bound"
    );
    let members = members_of(CONTAINER);
    assert_eq!(members.len(), 15, "the archive declares fifteen members");

    // Every member decodes as a `.zrd` document, so "the only passenger
    // string" is a claim about the whole archive rather than about the members
    // a reader happened to open.
    let mut passenger_strings: Vec<(String, String)> = Vec::new();
    for (member, member_bytes) in &members {
        let document = decode_zrd(member_bytes)
            .unwrap_or_else(|error| panic!("{CONTAINER}:{member} decodes: {error}"));
        let mut texts = Vec::new();
        walk_texts(&document, &mut texts);
        for text in texts {
            if text.to_lowercase().contains("passenger") {
                passenger_strings.push((member.clone(), text));
            }
        }
    }
    assert_eq!(
        passenger_strings,
        vec![("location.zrd".to_owned(), "Passenger_hangar".to_owned())],
        "the archive's whole passenger vocabulary is one location name"
    );

    // The location document is the four measured entries, in member order.
    let document = member_document(CONTAINER, "location.zrd");
    assert_eq!(
        locations_of(&document),
        LOCATIONS
            .iter()
            .map(|(name, position, heading)| (name.to_string(), *position, *heading))
            .collect::<Vec<_>>(),
        "a location is a name, a position and a heading: record data, not a guess"
    );

    // The member is the chapter's shared copy, byte for byte.
    let sibling = member_bytes("ZBD/C2/M02/zrdr.zbd", "location.zrd");
    assert_eq!(
        cs_assets::install::sha256(&sibling).to_hex(),
        LOCATION_MEMBER_SHA256,
        "the sibling mission carries the same location member"
    );
    assert_eq!(
        sibling,
        member_bytes(CONTAINER, "location.zrd"),
        "M06's location list is the chapter's shared list, not mission-authored content"
    );
}

// ---------------------------------------------------------------------------
// The owner's decrypted image: what the member's consumer is
// ---------------------------------------------------------------------------

/// **The image spells `location.zrd` as teleport data, not as an actor list.**
///
/// The image the loader accepts (its digest is the measured
/// `ORIGINAL_IMAGE_SHA256`, so these offsets describe the executable every
/// other static-analysis finding in this repository reads) carries, in one
/// `.data` run, the member's name five times beside a `Teleport` label, a
/// `Current Location` label, a `location.bak` backup name, the failure
/// diagnostics a writer emits, and the developer's own annotation that the
/// file "stores teleport data for this mission".
///
/// This is a **string-level** measurement and is recorded as one: no
/// instruction of the loader is traced here, so what the teleport feature
/// *does* with an entry stays unknown. What it settles is the question this
/// task asks — a `Passenger_hangar` entry is a teleport point, and the shipped
/// data gives it no actor, program or objective binding to predicate.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m06_b_fu2_the_image_spells_location_zrd_as_teleport_data() {
    let image = load_engine_image().unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        image.digest,
        original_image_digest(),
        "the loader only accepts the measured image, so the offsets below are its own"
    );
    let at = |offset: usize, expected: &str, what: &str| {
        let end = offset + expected.len();
        let found = image
            .bytes
            .get(offset..end)
            .unwrap_or_else(|| panic!("{what} lies inside the image"));
        assert_eq!(
            found,
            expected.as_bytes(),
            "{what} is spelled at file offset 0x{offset:x}"
        );
    };
    at(0x22_8d_10, "location.zrd", "the member name");
    at(
        0x22_8d_2c,
        "Cannot find location.zrd!",
        "the missing-member diagnostic",
    );
    at(0x22_8d_60, "Current Location", "the teleport UI label");
    at(
        0x22_8d_86,
        "location.zrd",
        "the member name, beside its comment",
    );
    at(
        0x22_8d_e9,
        "LOCATION.ZRD stores teleport data for this mission",
        "the developer annotation",
    );
    // The backup name, the write-failure diagnostic and the feature's own
    // label, in the same run: the feature writes the member back, which no
    // actor binding does.
    let run = &image.bytes[0x22_8d_00..0x22_8e_40];
    for needle in [
        "location.bak",
        "Could not open location.zrd for writing!",
        "Teleport",
    ] {
        assert!(
            run.windows(needle.len())
                .any(|window| window == needle.as_bytes()),
            "{needle} is in the same .data run as the member name"
        );
    }
}

// ---------------------------------------------------------------------------
// Retail: nothing else in the shipped data binds a passenger for M06
// ---------------------------------------------------------------------------

/// **No shipped record binds a passenger entity to M06.**
///
/// Three surfaces the entity could have lived on, measured rather than
/// assumed:
///
/// * **the mission's startup animations.** `startanims.zrd` fires
///   `player_setup`, `kktorch_burning1`, `kktorch_burning2` and
///   `place_the_goose` at `NEW_GAME_START` (and `player_setup` again at
///   `LOAD_GAME_START`). The library member that declares the crew animations
///   — seventeen `ON_CALL` definitions, every one driving the world node
///   `apassengers`, which is how a mission puts its gang aboard (M01 fires
///   `call_add_jack`) — declares none of the names M06 fires, so M06 puts no
///   crew aboard through that mechanism.
/// * **the chapter world's nodes.** `ZBD/C2/gamez.zbd` carries exactly one
///   `apassengers` and one `passall` node, and the world nodes M06's own
///   directives and targets name (`sprucegoose`, `propane`, `kkgate`,
///   `sghangar`, `tugandbarge01..04`) each resolve exactly once — so the
///   mission addresses what it uses, and it addresses neither passenger node.
/// * **the message table.** Of the census's mission-scoped archives, exactly
///   one carries `MSG_OBJ_PASSENGERHANGER`, and it is chapter one's instant
///   action; M06's own archive references fifteen message ids and none of them
///   is passenger-named.
///
/// The installation's passenger vocabulary that *does* exist is instant-action
/// and cosmetic: `ia.zrd` names `passenger_zeppelin` beside `cargo_zeppelin`
/// and `military_zeppelin`, and the library member above is crew cosmetics.
/// Neither is reachable from M06's data.
#[test]
#[ignore = "requires CS_GAME_DIR"]
#[allow(clippy::too_many_lines)]
fn accept_m06_b_fu2_no_shipped_record_binds_a_passenger_entity_for_m06() {
    // --- the startup animations the mission fires -------------------------
    let startanims = member_document(CONTAINER, "startanims.zrd");
    let mut fired: Vec<String> = Vec::new();
    walk_texts(&startanims, &mut fired);
    for key in ["NEW_GAME_START", "LOAD_GAME_START"] {
        assert!(
            fired.iter().any(|name| name == key),
            "the startup member spells {key}"
        );
    }
    let animation_names: Vec<&str> = fired
        .iter()
        .map(String::as_str)
        .filter(|name| !matches!(*name, "NEW_GAME_START" | "LOAD_GAME_START"))
        .collect();
    assert_eq!(
        animation_names,
        FIRED_STARTUP_ANIMATIONS.to_vec(),
        "M06's startup fires four animations and reloads one; none is a crew animation"
    );

    // --- the crew animation library, and what it drives -------------------
    let library_bytes =
        std::fs::read(game_dir().join(COMMON_CONTAINER)).expect("the library archive reads");
    assert_eq!(
        cs_assets::install::sha256(&library_bytes).to_hex(),
        COMMON_SHA256,
        "the library archive is the installation-wide one"
    );
    let passengers = member_bytes(COMMON_CONTAINER, PASSENGERS_MEMBER);
    assert_eq!(
        cs_assets::install::sha256(&passengers).to_hex(),
        PASSENGERS_MEMBER_SHA256,
        "the crew member's own bytes"
    );
    let document = decode_zrd(&passengers).expect("the crew member decodes");
    let definitions = crew_definitions(&document);
    let declared = definitions
        .iter()
        .map(|fields| {
            let mut name = None;
            let mut animation = None;
            let mut activation = None;
            for (key, value) in zrd_flat_fields(fields) {
                let text = |value: &ZrdValue| -> String {
                    value
                        .as_list()
                        .and_then(|items| items.first())
                        .and_then(ZrdValue::as_text)
                        .unwrap_or_else(|| panic!("{key} carries a text value"))
                        .to_owned()
                };
                match key {
                    "NAME" => name = Some(text(value)),
                    "ANIMATION_NAME" => animation = Some(text(value)),
                    "ACTIVATION" => activation = Some(text(value)),
                    _ => {}
                }
            }
            (
                name.expect("every definition names its object"),
                animation.expect("every definition names its animation"),
                activation.expect("every definition names its activation"),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        declared
            .iter()
            .map(|(_, animation, _)| animation.as_str())
            .collect::<Vec<_>>(),
        CREW_ANIMATIONS.to_vec(),
        "the library declares the seventeen crew animations, in order"
    );
    assert!(
        declared
            .iter()
            .all(|(name, _, activation)| name == CREW_NODE && activation == "ON_CALL"),
        "every crew animation drives the crew node and waits to be called"
    );
    assert!(
        animation_names
            .iter()
            .all(|fired| !CREW_ANIMATIONS.contains(fired)),
        "M06 fires none of the crew animations: {animation_names:?}"
    );

    // --- the chapter world's nodes ----------------------------------------
    let world_bytes =
        std::fs::read(game_dir().join(WORLD_CONTAINER)).expect("the world container reads");
    assert_eq!(
        cs_assets::install::sha256(&world_bytes).to_hex(),
        WORLD_SHA256,
        "the world container is the chapter's"
    );
    let label = WORLD_CONTAINER.to_owned();
    let nodes = read_gamez_nodes(
        &mut ParseContext::with_defaults(label.clone()),
        &world_bytes,
    )
    .unwrap_or_else(|error| panic!("{WORLD_CONTAINER}'s node array reads: {error}"));
    let count_of = |name: &str| nodes.nodes.iter().filter(|node| node.name == name).count();
    for node in MISSION_NODES {
        assert_eq!(
            count_of(node),
            1,
            "the world carries exactly one {node}, which is what the mission's directives name"
        );
    }
    assert_eq!(
        count_of(CREW_NODE),
        1,
        "the world carries exactly one crew node, which nothing in M06 names"
    );
    assert_eq!(
        count_of("passall"),
        1,
        "the world carries exactly one passenger-crowd node, which nothing in M06 names"
    );
    for member in ["objectives.zrd", "targets.zrd", "aiv.zrd", "location.zrd"] {
        let texts = member_texts(CONTAINER, member);
        for node in [CREW_NODE, "passall"] {
            assert!(
                !texts.iter().any(|text| text == node),
                "M06's {member} does not name the world's {node}"
            );
        }
    }

    // --- the message table -------------------------------------------------
    let census = survey_mission_control_programs(&game_dir()).expect("the census runs");
    let mut carriers: Vec<String> = Vec::new();
    for row in census.rows() {
        let bytes = std::fs::read(game_dir().join(&row.container))
            .unwrap_or_else(|error| panic!("{} reads: {error}", row.container));
        if find(&bytes, PASSENGER_MESSAGE_ID.as_bytes()).is_some() {
            carriers.push(row.container.clone());
        }
    }
    assert_eq!(
        carriers,
        vec![PASSENGER_MESSAGE_CARRIER.to_owned()],
        "the passenger objective string is chapter one's instant action, never M06's"
    );

    let mut referenced: Vec<String> = Vec::new();
    for (member, member_bytes) in members_of(CONTAINER) {
        let mut from = 0;
        while let Some(offset) = find(&member_bytes[from..], b"MSG_") {
            let start = from + offset;
            let end = start
                + member_bytes[start..]
                    .iter()
                    .take_while(|byte| byte.is_ascii_alphanumeric() || **byte == b'_')
                    .count();
            let id = String::from_utf8_lossy(&member_bytes[start..end]).into_owned();
            assert!(
                !id.to_lowercase().contains("passenger"),
                "{member} references no passenger message id: {id}"
            );
            referenced.push(id);
            from = end.max(start + 1);
        }
    }
    referenced.sort();
    referenced.dedup();
    assert_eq!(
        referenced,
        MISSION_MESSAGE_IDS.to_vec(),
        "M06's whole message vocabulary: crew names, two objective labels, four targets and its \
         three objective texts"
    );

    // The instant-action record's own vocabulary, which is where the
    // installation's one gameplay sense of "passenger" lives.
    let ia = member_texts(INSTANT_ACTION_CONTAINER, "ia.zrd");
    for class in ["cargo_zeppelin", "passenger_zeppelin", "military_zeppelin"] {
        assert!(
            ia.iter().any(|text| text == class),
            "the instant-action record names the {class} target class"
        );
    }
}

/// The `ANIMATION_DEFINITION` field lists of a crew document, in declaration
/// order: the flat `ANIMATION_LIST` walk, refusing a shape it does not know
/// rather than skipping it.
fn crew_definitions(document: &ZrdValue) -> Vec<ZrdValue> {
    let wrapper = document
        .as_list()
        .and_then(|outer| outer.first())
        .expect("the crew document is the measured one-element wrapper");
    let record = zrd_flat_fields(wrapper)
        .into_iter()
        .find(|(key, _)| *key == "ANIMATION_DEFINITIONS")
        .map(|(_, value)| value)
        .expect("the crew document is an ANIMATION_DEFINITIONS record");
    let definitions = zrd_flat_fields(record)
        .into_iter()
        .find(|(key, _)| *key == "ANIMATION_LIST")
        .map(|(_, value)| value.as_list().expect("the animation list is a list"))
        .expect("the crew document declares its animation list");
    let mut lists = Vec::new();
    let mut index = 0;
    while index < definitions.len() {
        assert_eq!(
            definitions[index],
            ZrdValue::Text("ANIMATION_DEFINITION".to_owned()),
            "the animation list alternates the marker and its field list"
        );
        let fields = definitions
            .get(index + 1)
            .expect("every marker is followed by its field list")
            .as_list()
            .expect("a definition's fields are a list")
            .to_vec();
        lists.push(ZrdValue::List(fields));
        index += 2;
    }
    lists
}

// ---------------------------------------------------------------------------
// Synthetic: the record shape the retail assertions read, in CI
// ---------------------------------------------------------------------------

/// **A location entry is a name plus two float triples, and carries nothing
/// else.**
///
/// The retail assertions above read M06's `location.zrd` as four
/// `(name, position, heading)` entries. That reading is pinned here on an
/// authored document in the same shape — the one-element wrapper around a flat
/// name/value list — so CI covers the grammar without original data, through
/// the production decoder as well as the production field walk, and so a
/// decoder change that let a location carry a kind, an actor or a spawn fails
/// here rather than silently re-pointing the retail assertions.
#[test]
fn accept_m06_b_fu2_a_location_entry_is_a_name_plus_two_float_triples() {
    let triple =
        |values: [f32; 3]| ZrdValue::List(values.into_iter().map(ZrdValue::Float).collect());
    let entry = |name: &str, position: [f32; 3], heading: [f32; 3]| {
        // The retail shape: a name, then one list holding the position triple
        // and the heading triple.
        vec![
            ZrdValue::Text(name.to_owned()),
            ZrdValue::List(vec![triple(position), triple(heading)]),
        ]
    };
    let document = ZrdValue::List(vec![ZrdValue::List(
        entry(
            "Airport_terminal",
            [-5249.0, 199.0, -4747.0],
            [-11.0, 22.0, 0.0],
        )
        .into_iter()
        .chain(entry(
            "Passenger_hangar",
            [-5177.0, 325.0, -6842.0],
            [-15.0, 9.0, 0.0],
        ))
        .collect(),
    )]);
    let expected = vec![
        (
            "Airport_terminal".to_owned(),
            [-5249.0, 199.0, -4747.0],
            [-11.0, 22.0, 0.0],
        ),
        (
            "Passenger_hangar".to_owned(),
            [-5177.0, 325.0, -6842.0],
            [-15.0, 9.0, 0.0],
        ),
    ];
    assert_eq!(
        locations_of(&document),
        expected,
        "a location is its name, its position and its heading"
    );

    // The same shape through the production decoder, so the retail assertions
    // read the bytes the way the decoder reports them.
    let mut bytes = Vec::new();
    encode_zrd(&document, &mut bytes);
    let decoded = decode_zrd(&bytes).expect("the authored location document decodes");
    assert_eq!(
        locations_of(&decoded),
        expected,
        "the production decoder carries the authored shape through unchanged"
    );
}

/// Encodes one authored value into `.zrd` bytes: the four measured tags, and
/// the list count the profiler stores (`children + 1`, F09's rule).
fn encode_zrd(value: &ZrdValue, bytes: &mut Vec<u8>) {
    match value {
        ZrdValue::Int(int) => {
            bytes.extend_from_slice(&1_u32.to_le_bytes());
            bytes.extend_from_slice(&int.to_le_bytes());
        }
        ZrdValue::Float(float) => {
            bytes.extend_from_slice(&2_u32.to_le_bytes());
            bytes.extend_from_slice(&float.to_bits().to_le_bytes());
        }
        ZrdValue::Text(text) => {
            bytes.extend_from_slice(&3_u32.to_le_bytes());
            bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
            bytes.extend_from_slice(text.as_bytes());
        }
        ZrdValue::List(children) => {
            bytes.extend_from_slice(&4_u32.to_le_bytes());
            bytes.extend_from_slice(&((children.len() + 1) as u32).to_le_bytes());
            for child in children {
                encode_zrd(child, bytes);
            }
        }
    }
}
