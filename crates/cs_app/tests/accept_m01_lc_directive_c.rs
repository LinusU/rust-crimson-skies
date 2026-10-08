//! Acceptance for `M01-LC-DIRECTIVE-C` (#681): the findings document
//! `docs/findings/2026-10-06-m01-lc-directive-c-ai-world-and-animation-directives.md`
//! records what M01's AI, world and animation directives **do**, measured from
//! the native handlers in `$CS_ENGINE_IMAGE` and cross-checked
//! against the arguments the installation ships.
//!
//! Three independent production observations are pinned, so the document cannot
//! drift away from either the data or the code without failing:
//!
//! 1. the **scope vocabulary** — the keys of this stage's scope that M01's
//!    control program actually spells, read from the production census
//!    ([`survey_mission_control_programs`]);
//! 2. the **argument shapes** those sites spell, from the same census;
//! 3. the **shipped arguments** themselves — every site of every scoped key with
//!    its decoded values, read from the member the production control-member
//!    rule selects ([`read_control_member`]) through the production `.zrd`
//!    decoder. This is the data the document's "what M01 spells" column quotes,
//!    so a decoding or census regression fails here.
//!
//! All three need `CS_GAME_DIR`, so they are ignored without it and run with
//! `--include-ignored`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use cs_app::mission_control::{read_control_member, survey_mission_control_programs};
use cs_content::stunts::{ZrdValue, objective_record, zrd_flat_fields};

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR: retail acceptance needs the install"),
    )
}

/// The scoped vocabulary `M01-LC-DIRECTIVE-C` measures, in census order.
///
/// Seven of M01's 43 directive keys act on actors, on the world or on the
/// animation system; the other 36 belong to stages `M01-LC-DIRECTIVE-B`
/// (objective lifecycle and targets) and `M01-LC-DIRECTIVE-D` (sound, help and
/// timers). The six sibling keys this stage also measured but which M01 never
/// spells — `WAKEUP_TURRETS`, `WARP_VEHICLE`, `SET_AI_TEAM`,
/// `SET_AI_ATTACK_RADIUS`, `START_TAXI`, `SLEEP_ANIM` — are named in the
/// findings document with their handlers and are absent from this list, so a
/// census that started reporting them (or dropped one of the seven) fails.
const SCOPED_KEYS_M01_SPELLS: &[&str] = &[
    "ANIM_STATE",
    "SET_AI_NET",
    "TRAVELERS",
    "WAKEUP_ENEMIES",
    "WAKEUP_GENERATOR",
    "WAKEUP_ZEP_TURRETS",
    "WAKE_ANIM",
];

/// The scoped keys the native parser handles but M01 never spells. Asserted as
/// *absent* from the census: the document measures them from the executable, and
/// this stage does not claim M01 uses them.
const SCOPED_KEYS_M01_DOES_NOT_SPELL: &[&str] = &[
    "SET_AI_ATTACK_RADIUS",
    "SET_AI_TEAM",
    "SLEEP_ANIM",
    "START_TAXI",
    "WAKEUP_TURRETS",
    "WARP_VEHICLE",
];

fn scoped_keys_of_census() -> Vec<String> {
    let census = survey_mission_control_programs(&game_dir()).expect("census runs on the install");
    let record = census
        .row("zbd/c1c/m01")
        .expect("M01 is present")
        .record()
        .expect("M01 declares a control program");
    record
        .keys()
        .iter()
        .map(|key| key.key.clone())
        .filter(|key| {
            SCOPED_KEYS_M01_SPELLS.contains(&key.as_str())
                || SCOPED_KEYS_M01_DOES_NOT_SPELL.contains(&key.as_str())
        })
        .collect()
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_directive_c_the_scoped_vocabulary_is_exactly_the_seven_keys_m01_spells() {
    assert_eq!(
        scoped_keys_of_census(),
        SCOPED_KEYS_M01_SPELLS
            .iter()
            .map(|key| (*key).to_owned())
            .collect::<Vec<String>>(),
        "M01 spells exactly these keys of this stage's scope, in census order"
    );
}

/// The argument shapes the document's per-key table records for the scoped
/// keys, as the production census measures them from `objectives.zrd`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_directive_c_each_scoped_keys_argument_shapes_match_the_record() {
    let census = survey_mission_control_programs(&game_dir()).expect("census runs on the install");
    let record = census
        .row("zbd/c1c/m01")
        .expect("M01 is present")
        .record()
        .expect("M01 declares a control program");
    let measured: BTreeMap<&str, Vec<String>> = record
        .keys()
        .iter()
        .filter(|key| SCOPED_KEYS_M01_SPELLS.contains(&key.key.as_str()))
        .map(|key| {
            (
                key.key.as_str(),
                key.shapes
                    .iter()
                    .map(|(shape, count)| format!("{}x{}", shape.label(), count))
                    .collect(),
            )
        })
        .collect();
    let documented: BTreeMap<&str, &[&str]> = BTreeMap::from([
        ("ANIM_STATE", &["[text,[text,[text],text,[text]]]x3"][..]),
        (
            "SET_AI_NET",
            &["[[text,text]]x2", "[[text,text],[text,text]]x1"][..],
        ),
        ("TRAVELERS", &["[text,text,text,float,int]x1"][..]),
        ("WAKEUP_ENEMIES", &["[text,text]x1"][..]),
        ("WAKEUP_GENERATOR", &["[text,int]x4"][..]),
        ("WAKEUP_ZEP_TURRETS", &["[text]x3"][..]),
        ("WAKE_ANIM", &["[text]x5"][..]),
    ]);
    assert_eq!(
        measured.len(),
        documented.len(),
        "the documented shape table covers every scoped key"
    );
    for (key, shapes) in &documented {
        let expected: Vec<String> = shapes.iter().map(|shape| (*shape).to_owned()).collect();
        assert_eq!(
            measured.get(key),
            Some(&expected),
            "measured argument shapes for {key}"
        );
    }
}

/// Every scoped directive site M01 spells, with its decoded arguments, exactly
/// as the findings document quotes them.
///
/// The renderer is deliberately total and lossless for the four `.zrd` node
/// kinds: a rounded float would let a decoding change hide behind it, and a
/// nested list is rendered as nested brackets because two of these keys
/// (`SET_AI_NET`, `ANIM_STATE`) spell lists the mission IR cannot carry.
fn render(value: &ZrdValue) -> String {
    match value {
        ZrdValue::Int(v) => format!("int {v}"),
        ZrdValue::Float(v) => format!("float {v:?}"),
        ZrdValue::Text(v) => format!("text {v}"),
        ZrdValue::List(children) => {
            let inner: Vec<String> = children.iter().map(render).collect();
            format!("[{}]", inner.join(", "))
        }
    }
}

fn shipped_sites() -> Vec<(String, String, String)> {
    let (document, _row) =
        read_control_member(&game_dir(), "zbd/c1c/m01").expect("M01's control member decodes");
    let mut sites = Vec::new();
    for (block, value) in zrd_flat_fields(objective_record(&document)) {
        if !block.starts_with("OBJECTIVE") {
            continue;
        }
        for (key, args) in zrd_flat_fields(value) {
            if SCOPED_KEYS_M01_SPELLS.contains(&key) {
                sites.push((block.to_owned(), key.to_owned(), render(args)));
            }
        }
    }
    sites
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_directive_c_the_shipped_arguments_of_the_scoped_keys_match_the_record() {
    let documented: Vec<(String, String, String)> = [
        ("OBJECTIVE1", "WAKEUP_ZEP_TURRETS", "[text piratezep]"),
        (
            "OBJECTIVE2",
            "SET_AI_NET",
            "[[text devastator_2, text Bravo2], [text devastator_3, text Charlie2]]",
        ),
        (
            "OBJECTIVE3",
            "TRAVELERS",
            "[text player, text APPROACHING, text workersvoyagezep, float 700.0, int 1]",
        ),
        (
            "OBJECTIVE5",
            "WAKEUP_ZEP_TURRETS",
            "[text workersvoyagezep]",
        ),
        (
            "OBJECTIVE9",
            "WAKEUP_GENERATOR",
            "[text workersvoyagezep, int 3]",
        ),
        ("OBJECTIVE11", "WAKE_ANIM", "[text activate_dropoff_node]"),
        (
            "OBJECTIVE11",
            "ANIM_STATE",
            "[text ANIM, [text NAME, [text wv_drop_copilot], text STATE, [text RUNNING]]]",
        ),
        (
            "OBJECTIVE13",
            "WAKEUP_ENEMIES",
            "[text bsfury_1, text blackswanzep]",
        ),
        ("OBJECTIVE13", "WAKE_ANIM", "[text fadein_bszep]"),
        (
            "OBJECTIVE13",
            "WAKEUP_GENERATOR",
            "[text blackswanzep, int 2]",
        ),
        ("OBJECTIVE13", "WAKEUP_ZEP_TURRETS", "[text blackswanzep]"),
        ("OBJECTIVE15", "WAKE_ANIM", "[text activate_pickup_node]"),
        (
            "OBJECTIVE15",
            "ANIM_STATE",
            "[text ANIM, [text NAME, [text wv_pickup_copilot], text STATE, [text EXECUTED]]]",
        ),
        ("OBJECTIVE18", "WAKE_ANIM", "[text pzhomebase]"),
        (
            "OBJECTIVE18",
            "ANIM_STATE",
            "[text ANIM, [text NAME, [text hooked_to_klondike], text STATE, [text EXECUTED]]]",
        ),
        (
            "OBJECTIVE19",
            "SET_AI_NET",
            "[[text bsfury_1, text BlackSwan]]",
        ),
        (
            "OBJECTIVE22",
            "SET_AI_NET",
            "[[text blackswanzep, text SwanZep2]]",
        ),
        (
            "OBJECTIVE27",
            "WAKEUP_GENERATOR",
            "[text workersvoyagezep, int 3]",
        ),
        (
            "OBJECTIVE45",
            "WAKEUP_GENERATOR",
            "[text blackswanzep, int 4]",
        ),
        ("OBJECTIVE49", "WAKE_ANIM", "[text activate_dropoff_node]"),
    ]
    .map(|(block, key, args)| (block.to_owned(), key.to_owned(), args.to_owned()))
    .to_vec();

    assert_eq!(
        shipped_sites(),
        documented,
        "the shipped arguments of every scoped directive site"
    );
}
