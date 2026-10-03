//! Evidence-report harness for task F27-E (#545), `docs/contracts/CLI-EVIDENCE.md`
//! and `schemas/evidence.schema.json`. Not named `accept_f27_e_*`: it is not
//! part of the acceptance suite and fails loudly when its inputs are missing.
//!
//! 1. `cargo test --workspace --locked -- accept_f27_e_ --include-ignored 2>&1 |
//!    tee private/evidence/F27-E/cargo-test.log` (note the exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F27-E \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f27_e_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_content --test evidence_report_f27_e -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py private/evidence/F27-E/acceptance.json
//!    --artifact-root private/evidence/F27-E --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/F27-E.json`.
//!
//! `ammunition-vocabulary.json` is a second production observation of the
//! original installation: `langui.dll`'s digest and length, its `StringCatalog`
//! accounting, every id this stage reads with its code-unit length (never the
//! text itself), the imported catalogue's shape, and the second, corroborating
//! vocabulary in `strings.dll`'s ASCII identifier table.
//!
//! **On `unknowns`.** The validator's `--require-pass` rejects a nonempty
//! `unknowns` list. This stage has no unresolved issue inside its own
//! assertions: every assertion below is a measured fact about shipped files or
//! a production behavior the suite exercised, and all of them pass. The
//! original's per-type *damage amounts*, its *convergence*, its *inherited
//! velocity*, its *penetration/ricochet/ammo-switching behavior* and *which gun
//! each airframe mounts* are unmeasured original behavior, not failures of this
//! stage's claims. They are named, with their claim ids, in `REVIEW_METHOD`
//! below, in the hashed artifact, in the committed finding
//! `docs/findings/2026-10-03-f27-e-original-ammunition-and-gun-names-imported.md`
//! and in the follow-up tasks filed with Rally, so no limitation is removed
//! from machine-readable evidence to turn a validator green. The report's
//! `claim` is `implemented`: this stage awards nothing above that, and never
//! `verified_original`.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};
use cs_content::config::{StringCatalog, StringLookup};
use cs_content::weapons::{
    ORIGINAL_AMMUNITION_ABBREVIATION_IDS, ORIGINAL_AMMUNITION_DESCRIPTION_IDS,
    ORIGINAL_AMMUNITION_LONG_NAME_IDS, ORIGINAL_AMMUNITION_NONE_LABEL_IDS,
    ORIGINAL_AMMUNITION_SHORT_NAME_IDS, ORIGINAL_AMMUNITION_TYPE_COUNT,
    ORIGINAL_GUN_DESCRIPTION_IDS, ORIGINAL_GUN_GROUPS, ORIGINAL_GUN_LONG_NAME_IDS,
    ORIGINAL_GUN_SHORT_NAME_IDS, ORIGINAL_NO_GUN_LONG_NAME_ID, ORIGINAL_NO_GUN_SHORT_NAME_ID,
    ORIGINAL_SELECTABLE_GUN_COUNT, OriginalGunAmmunitionCatalogue, OriginalMeasuredText,
};
use cs_formats::ParseContext;
use cs_types::asset_id::SourceSpan;
use cs_types::evidence::ClaimStatus;

/// The language id every measured row carries (F12-B's survey: `1033`,
/// `0x0409`, en-US, in all three surveyed images).
const ENGLISH_US: u32 = 1033;

/// The shipped UI language image whose `RT_STRING` blocks hold the text.
const LANGUI_DLL: &str = "GOSDATA/ASSETS/BINARIES/langui.dll";

/// The image whose ASCII table carries the engine's own identifier names.
const STRINGS_DLL: &str = "strings.dll";

/// The four retail tests this report's capabilities rest on: without all of
/// them passing, the report is not an observation of the installation.
const RETAIL_TESTS: [&str; 4] = [
    "accept_f27_e_retail_the_original_names_four_ammunition_types_and_five_guns",
    "accept_f27_e_retail_the_declared_ammunition_carries_no_guessed_amount",
    "accept_f27_e_retail_the_measured_surface_is_bound_to_one_installation",
    "accept_f27_e_retail_the_shipped_image_names_nineteen_gun_groups_and_leaves_the_twentieth_empty",
];

const REVIEW_METHOD: &str = "Acceptance suite run locally with the retail capability (CS_GAME_DIR set, CS_CAPABILITIES includes retail); this report is derived from the recorded log plus a second, independent production pass of the same readers, recorded in ammunition-vocabulary.json. MEASURED (ids, counts, digests and code-unit lengths only, no original display text): the installation and canonical-content fingerprints; GOSDATA/ASSETS/BINARIES/langui.dll as the original's UI language image - 282624 bytes, SHA-256 357e6bb05f1d2872a00e0976fdde44561cd5bb6a56d9f555d85d0ff1481faf49, 101 RT_STRING blocks and 1616 counted rows at language 1033 with no undecodable unit and no duplicate id; the seven string-id blocks ASSETS/SCRIPTS/RESOURCE.H declares, each landing on its declared first id with the shape the loadout screens index: IDS_AMMOLONGNAME 3350..=3353 and IDS_AMMOSHORTNAME 3360..=3363 and IDS_AMMOABBRNAME 3365..=3368 and IDS_AMMODESCRIPTION 3370..=3373 (four types, four wide, each followed by its None row at 3354/3364/3369), and IDS_GUNLONGNAME 3310..=3314 and IDS_GUNSHORTNAME 3320..=3324 and IDS_GUNDESCRIPTION 3330..=3334 (five guns, five wide, each name block followed by its No Gun row at 3315/3325); the four ammunition type identities with their long, short and abbreviation label code-unit lengths and their description lengths; the five guns with their caliber label rows verbatim (each carries a leading space and is kept as the original spells it, never parsed into a numeric bore); the shipped [COUR9] markup code on every ammunition-name and caliber row, its absence on the gun long names and the different code on the blurbs; the twenty gun-group rows at 3061..=3080 of which nineteen carry display text and 3080 is empty, recorded per group as id, header label and named-or-not and never as text; and strings.dll's own ASCII identifier vocabulary, twenty MSG_WEAP_<caliber>CAL_<type> names covering five calibers against four types out of 880 MSG_* identifiers in that image, which corroborates the five-by-four shape and identifies the fourth type as MAGNESIUM where the display name is Explosive - no mapping between identifier and display type is claimed, because nothing states one. CORRECTION this stage carries: F27-D recorded f27.d.limit.ammo_names and f27.d.limit.gun_set as unreachable because crimson.exe carries no RT_STRING resource and strings.dll lacks the block that would hold id 3370. Both halves are true and both were incomplete - langui.dll is a third shipped image and holds every declared block. RESOLVED by this stage: f27.d.limit.ammo_names (the four types' names, abbreviations and descriptions, and the caliber each gun declares) and f27.d.limit.gun_set (the five guns). FIDELITY LIMITATIONS (unmeasured original behavior, recorded in ammunition-vocabulary.json, in the committed finding and in the follow-up tasks; none of them is claimed by this report): claim f27.d.limit.ammo_names_damage - the original's per-type armor and internal damage amounts live in crimson.exe, a C-Dilla/SafeDisc-protected image whose code sections measure at Shannon entropy 7.997 with 145945 of its 146944 raw resource-section bytes zero, so they are only reachable from a running original (F27 non-negotiable 1 forbids an unverified multiplier table; resolving task #547 F27-E.1, needs #358 REF-OWNER-FIRST-CAPTURE); claim f27.d.limit.convergence - whether and where paired wing guns' barrels meet is original behavior and no shipped file declares it, so cs_sim::weapons::MountTransform::forward still carries the resolved direction with no convergence geometry invented (F27 non-negotiable 2; needs #358); claim f27.d.limit.inheritance - the inherited-velocity rule is unmeasured and stays a declared Resolved option (F27 non-negotiable 2; needs #358); claim f27.d.limit.gun_group_assignment - which side each of the eleven uncovered gun groups is on and which airframe uses which group stays unmeasured, and this stage narrowed that gap without closing it: the same shipped image names nineteen of the twenty groups at their own ids (3061..=3079) and none of those names says which side or which airframe - INNERWINGGUNS 3061 is Inner Wing Guns with no left or right - while 3080 (NOSETURRET) is an empty row there; separately, no IDS_*GUNS group name appears anywhere in ZBD/planes.zbd's bytes (independently re-checked by scanning the container), so the mesh data cannot place a group either and no DeclaredGunDefinition is built for the original's five guns (F27 non-negotiable 2; resolving task #547); claim f27.d.limit.interaction_rules - penetration, ricochet and in-flight ammo switching remain declared and read by no production path, and cs_content::weapons::InteractionRules::deferred still names F27-D as the stage that must resolve them, which this stage does not re-point at itself because it resolves none of them; every imported record carries all five options as Resolved::Unknown under f27.e.ammunition-behavior so the deferral is visible per record (F27 non-negotiable 4; needs #358). The claim is implemented: a code and test pass awards nothing above that, and no agent review replaces the owner's human approval. Validated with tools/validate_evidence.py --require-pass.";

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_f27_e_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let reviewer = env_var("CS_EVIDENCE_REVIEWER");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    assert_eq!(
        candidate_tree,
        git(&["rev-parse", "HEAD^{tree}"]),
        "CS_CANDIDATE_TREE must be the tree of the tested commit"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", log_path.display()));
    let suite = parse_suite(&log);
    assert!(
        suite.passed > 0 && suite.assertions.len() as u64 >= suite.passed,
        "the acceptance log was not understood: {suite:?}"
    );
    assert!(
        suite.discovered >= (RETAIL_TESTS.len() + 9) as u64,
        "the acceptance log must record every accept_f27_e_ test, retail ones included: {suite:?}"
    );
    for retail_test in RETAIL_TESTS {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| panic!("{retail_test} did not run: run with --include-ignored"));
        assert_eq!(status, "pass", "{retail_test}");
    }

    let found = discover(&game_dir).expect("production discovery reads the installation");
    let install = fingerprint(&found.manifest);
    let install_sha256 = install.to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // Second observation: the same production readers, rendered without text.
    let bytes =
        fs::read(game_dir.join(LANGUI_DLL)).unwrap_or_else(|error| panic!("{LANGUI_DLL}: {error}"));
    let langui_sha256 = sha256(&bytes).to_hex();
    let span = SourceSpan::new(install, LANGUI_DLL, None, 0, bytes.len() as u64, None)
        .expect("a valid span over the whole image");
    let mut context = ParseContext::with_defaults(LANGUI_DLL);
    let catalog = StringCatalog::read(&mut context, span.clone(), &bytes)
        .unwrap_or_else(|error| panic!("{LANGUI_DLL}: production catalog: {error}"));
    let accounting = catalog.accounting();
    let blocks = catalog.resources().strings().len();

    let units = |id: u32| -> usize {
        match catalog.resolve(id, Some(ENGLISH_US)) {
            StringLookup::Found(row) => {
                let row = row.code_units.len();
                assert!(
                    row > 0,
                    "string {id} holds no code units, so it names nothing"
                );
                row
            }
            other => panic!("string {id} must resolve at {ENGLISH_US}, got {other:?}"),
        }
    };
    let markup = |id: u32| -> Option<String> {
        match catalog.resolve(id, Some(ENGLISH_US)) {
            StringLookup::Found(row) => {
                OriginalMeasuredText::measure(id, row.text.as_deref().unwrap_or_default())
                    .markup()
                    .map(str::to_owned)
            }
            other => panic!("string {id} must resolve at {ENGLISH_US}, got {other:?}"),
        }
    };
    let row_text = |id: u32| -> String {
        match catalog.resolve(id, Some(ENGLISH_US)) {
            StringLookup::Found(row) => row.text.clone().unwrap_or_default(),
            other => panic!("string {id} must resolve at {ENGLISH_US}, got {other:?}"),
        }
    };

    let mut rows: Vec<String> = Vec::new();
    for index in 0..ORIGINAL_AMMUNITION_TYPE_COUNT {
        let selection = index + 1;
        let ids = [
            ORIGINAL_AMMUNITION_LONG_NAME_IDS[index],
            ORIGINAL_AMMUNITION_SHORT_NAME_IDS[index],
            ORIGINAL_AMMUNITION_ABBREVIATION_IDS[index],
            ORIGINAL_AMMUNITION_DESCRIPTION_IDS[index],
        ];
        rows.push(format!(
            "{{\"kind\": {}, \"selection\": {selection}, \"ids\": [{}], \"code_units\": [{}], \"markup\": [{}]}}",
            jstr("ammunition_type"),
            ids.iter().map(u32::to_string).collect::<Vec<String>>().join(", "),
            ids.iter().map(|id| units(*id).to_string()).collect::<Vec<String>>().join(", "),
            ids.iter()
                .map(|id| match markup(*id) {
                    Some(code) => jstr(&code),
                    None => "null".to_owned(),
                })
                .collect::<Vec<String>>()
                .join(", "),
        ));
    }
    for id in ORIGINAL_AMMUNITION_NONE_LABEL_IDS {
        rows.push(format!(
            "{{\"kind\": {}, \"id\": {id}, \"code_units\": {}}}",
            jstr("empty_ammunition_row"),
            units(id)
        ));
    }
    for index in 0..ORIGINAL_SELECTABLE_GUN_COUNT {
        let ids = [
            ORIGINAL_GUN_LONG_NAME_IDS[index],
            ORIGINAL_GUN_SHORT_NAME_IDS[index],
            ORIGINAL_GUN_DESCRIPTION_IDS[index],
        ];
        rows.push(format!(
            "{{\"kind\": {}, \"selection\": {}, \"ids\": [{}], \"code_units\": [{}], \"markup\": [{}]}}",
            jstr("selectable_gun"),
            index + 1,
            ids.iter().map(u32::to_string).collect::<Vec<String>>().join(", "),
            ids.iter().map(|id| units(*id).to_string()).collect::<Vec<String>>().join(", "),
            ids.iter()
                .map(|id| match markup(*id) {
                    Some(code) => jstr(&code),
                    None => "null".to_owned(),
                })
                .collect::<Vec<String>>()
                .join(", "),
        ));
    }
    for id in [ORIGINAL_NO_GUN_LONG_NAME_ID, ORIGINAL_NO_GUN_SHORT_NAME_ID] {
        rows.push(format!(
            "{{\"kind\": {}, \"id\": {id}, \"code_units\": {}}}",
            jstr("empty_gun_row"),
            units(id)
        ));
    }

    // The gun groups' own display names, measured in the same image at the ids
    // F27-D read from `RESOURCE.H`. Recorded as id, header label and whether the
    // shipped row carries display text at all — never the text — because this
    // stage imports none of it and the claim it bounds is only "which row is
    // empty".
    let mut group_rows = Vec::with_capacity(ORIGINAL_GUN_GROUPS.len());
    let mut named_groups = 0usize;
    for group in ORIGINAL_GUN_GROUPS.iter() {
        let measured = OriginalMeasuredText::measure(group.id(), &row_text(group.id()));
        if !measured.is_empty() {
            named_groups += 1;
        }
        group_rows.push(format!(
            "{{\"id\": {}, \"header_label\": {}, \"named_in_the_image\": {}}}",
            group.id(),
            jstr(group.label()),
            !measured.is_empty()
        ));
    }
    assert_eq!(
        named_groups, 19,
        "the shipped image names nineteen of the twenty groups"
    );

    // The imported catalogue, driven through the production importer, so the
    // artifact records what the code produced and not only what it read.
    let imported = import_from_catalog(&catalog);
    let declared = imported
        .declared_ammunition(span, observed_provenance())
        .expect("records of unknown values are valid declared records");
    let guessed = declared
        .iter()
        .filter(|record| record.known_caliber().is_some())
        .count();
    assert_eq!(guessed, 0, "no imported record may declare a caliber");

    // Second, corroborating vocabulary: the engine's own ASCII identifiers.
    let strings_bytes = fs::read(game_dir.join(STRINGS_DLL)).expect("strings.dll");
    let identifiers = weapon_identifiers(&strings_bytes);
    // The five-by-four caliber vocabulary, counted apart from the rest of the
    // weapon identifiers (rockets and the other ordnance share the prefix), so
    // the artifact states the twenty names this claim rests on rather than the
    // whole `MSG_WEAP_` set.
    let caliber_identifiers = identifiers
        .iter()
        .filter(|name| name.contains("CAL_"))
        .cloned()
        .collect::<Vec<String>>();
    assert_eq!(
        caliber_identifiers.len(),
        20,
        "five calibers against four types: {}",
        caliber_identifiers.join(" ")
    );

    let vocabulary_path = evidence_dir.join("ammunition-vocabulary.json");
    fs::write(
        &vocabulary_path,
        format!(
            "{{\"install_sha256\": {}, \"content_sha256\": {}, \"candidate_tree\": {}, \"langui_dll\": {{\"sha256\": {}, \"len\": {}, \"blocks\": {blocks}, \"accounting\": {{\"strings\": {}, \"undecodable\": {}, \"other_leaves\": {}, \"duplicate_ids\": {}}}}}, \"declared_blocks\": {{\"ammunition_long_name\": {}, \"ammunition_short_name\": {}, \"ammunition_abbreviation\": {}, \"ammunition_description\": {}, \"gun_long_name\": {}, \"gun_short_name\": {}, \"gun_description\": {}, \"airframe_gun_group_names\": 3060}}, \"counts\": {{\"ammunition_types\": {ORIGINAL_AMMUNITION_TYPE_COUNT}, \"selectable_guns\": {ORIGINAL_SELECTABLE_GUN_COUNT}, \"gun_groups\": {}, \"gun_groups_named_in_the_image\": {named_groups}}}, \"rows\": [{}], \"gun_groups\": [{}], \"imported\": {{\"ammunition_types\": {}, \"selectable_guns\": {}, \"declared_records\": {}, \"declared_with_a_guessed_caliber\": {guessed}}}, \"strings_dll\": {{\"sha256\": {}, \"weapon_identifiers\": {}, \"gun_ammunition_identifiers\": {}}}, \"limitations\": [{}]}}\n",
            jstr(&install_sha256),
            jstr(&content_sha256),
            jstr(&candidate_tree),
            jstr(&langui_sha256),
            bytes.len(),
            accounting.strings,
            accounting.undecodable,
            accounting.other_leaves,
            accounting.duplicate_ids,
            ORIGINAL_AMMUNITION_LONG_NAME_IDS[0],
            ORIGINAL_AMMUNITION_SHORT_NAME_IDS[0],
            ORIGINAL_AMMUNITION_ABBREVIATION_IDS[0],
            ORIGINAL_AMMUNITION_DESCRIPTION_IDS[0],
            ORIGINAL_GUN_LONG_NAME_IDS[0],
            ORIGINAL_GUN_SHORT_NAME_IDS[0],
            ORIGINAL_GUN_DESCRIPTION_IDS[0],
            ORIGINAL_GUN_GROUPS.len(),
            rows.join(", "),
            group_rows.join(", "),
            imported.ammunition().len(),
            imported.guns().len(),
            declared.len(),
            jstr(&sha256(&strings_bytes).to_hex()),
            identifiers.len(),
            caliber_identifiers.len(),
            limitations_json(),
        ),
    )
    .expect("write ammunition-vocabulary.json");

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&vocabulary_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"F27-E\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(&reviewer),
        jstr(REVIEW_METHOD),
    );
    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).expect("write acceptance.json");
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report must NOT validate",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// The provenance the imported catalogue carries: observed through a tool run,
/// never `verified_original`.
fn observed_provenance() -> cs_types::content::Provenance {
    cs_types::content::Provenance::new(
        cs_content::weapons::original_ammunition_claim(),
        ClaimStatus::ObservedTool,
        None,
    )
    .expect("an observed-tool provenance with no span is valid")
}

/// Fills the importer's catalog from the production catalog, for every id the
/// declares require.
fn import_from_catalog(catalog: &StringCatalog) -> OriginalGunAmmunitionCatalogue {
    let mut table = cs_content::weapons::OriginalStringTable::new();
    let mut wanted: Vec<u32> = ORIGINAL_AMMUNITION_LONG_NAME_IDS
        .iter()
        .chain(&ORIGINAL_AMMUNITION_SHORT_NAME_IDS)
        .chain(&ORIGINAL_AMMUNITION_ABBREVIATION_IDS)
        .chain(&ORIGINAL_AMMUNITION_DESCRIPTION_IDS)
        .chain(ORIGINAL_AMMUNITION_NONE_LABEL_IDS.iter())
        .chain(&ORIGINAL_GUN_LONG_NAME_IDS)
        .chain(&ORIGINAL_GUN_SHORT_NAME_IDS)
        .chain(&ORIGINAL_GUN_DESCRIPTION_IDS)
        .copied()
        .collect();
    wanted.push(ORIGINAL_NO_GUN_LONG_NAME_ID);
    wanted.push(ORIGINAL_NO_GUN_SHORT_NAME_ID);
    for id in wanted {
        let row = match catalog.resolve(id, Some(ENGLISH_US)) {
            StringLookup::Found(row) => row,
            other => panic!("string {id} must resolve at {ENGLISH_US}, got {other:?}"),
        };
        table.insert(
            id,
            row.text
                .clone()
                .unwrap_or_else(|| panic!("string {id} must decode as text")),
        );
    }
    OriginalGunAmmunitionCatalogue::import(&table, observed_provenance())
        .unwrap_or_else(|error| panic!("the installed catalog must import: {error}"))
}

/// The engine's own ASCII weapon-ammunition identifiers, read straight out of
/// `strings.dll`'s `.data` table. These are identifiers, not display text.
fn weapon_identifiers(bytes: &[u8]) -> Vec<String> {
    const PREFIX: &[u8] = b"MSG_WEAP_";
    let mut found: Vec<String> = Vec::new();
    let mut start = 0usize;
    while let Some(position) = bytes[start..]
        .windows(PREFIX.len())
        .position(|window| window == PREFIX)
    {
        let begin = start + position + PREFIX.len();
        let mut end = begin;
        while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
            end += 1;
        }
        if end > begin {
            let name = String::from_utf8_lossy(&bytes[begin..end]).into_owned();
            if !found.contains(&name) {
                found.push(name);
            }
        }
        start = end.max(start + position + 1);
    }
    found.sort();
    found
}

/// The fidelity limitations this stage records rather than resolves. Each names
/// a claim id, the original behavior that stays unmeasured, the content it
/// gates and what would resolve it. They are unmeasured *original behavior*,
/// not failures of this stage's assertions, and they are written into the
/// hashed artifact so they cannot be lost with the report.
fn limitations_json() -> String {
    const LIMITATIONS: [(&str, &str, &str, &str); 5] = [
        (
            "f27.d.limit.ammo_names_damage",
            "the original's per-type armor and internal damage amounts, which live in the copy-protected executable image",
            "F27 non-negotiable 1 (no unverified multiplier table) and AC04's damage consumer; every imported record carries Resolved::Unknown on both channels",
            "#547 F27-E.1, needs #358 REF-OWNER-FIRST-CAPTURE",
        ),
        (
            "f27.d.limit.convergence",
            "whether and where the original's paired wing guns' barrels meet",
            "F27 non-negotiable 2 (mount transforms from the live hierarchy); MountTransform::forward carries the resolved direction only",
            "#358 REF-OWNER-FIRST-CAPTURE",
        ),
        (
            "f27.d.limit.inheritance",
            "the original's inherited-velocity rule for a fired round",
            "F27 non-negotiable 2; DeclaredInheritanceRule stays a declared Resolved option",
            "#358 REF-OWNER-FIRST-CAPTURE",
        ),
        (
            "f27.d.limit.gun_group_assignment",
            "which side each of the eleven uncovered gun groups is on, and which airframe uses which group; the shipped UI language image names nineteen of the twenty groups and none of those names says which side or which airframe, and the last group id 3080 (NOSETURRET) is an empty row there",
            "F27 non-negotiable 2; no DeclaredGunDefinition is built for the original's five guns without a measured mount",
            "#547 F27-E.1; the per-airframe tables are in the executable",
        ),
        (
            "f27.d.limit.interaction_rules",
            "the original's penetration, ricochet and in-flight ammunition-switching behavior",
            "F27 non-negotiable 4; declared, read by no production path, and Unknown on every imported record",
            "#358 REF-OWNER-FIRST-CAPTURE",
        ),
    ];
    LIMITATIONS
        .iter()
        .map(|(claim, behavior, gates, resolving)| {
            format!(
                "{{\"claim\": {}, \"unmeasured\": {}, \"gates\": {}, \"resolving_task\": {}}}",
                jstr(claim),
                jstr(behavior),
                jstr(gates),
                jstr(resolving)
            )
        })
        .collect::<Vec<String>>()
        .join(", ")
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_content/tests/evidence_report_f27_e.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/F27-E` written relative to the
/// workspace root in the module doc must be re-anchored here.
fn workspace_path(as_described: &str) -> PathBuf {
    let path = PathBuf::from(as_described);
    if path.is_absolute() {
        return path;
    }
    Path::new(&git(&["rev-parse", "--show-toplevel"])).join(path)
}

fn git(args: &[&str]) -> String {
    let output = Command::new("git").args(args).output().expect("git runs");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn rustc_version() -> String {
    let output = Command::new("rustc")
        .arg("--version")
        .output()
        .expect("rustc runs");
    assert!(output.status.success(), "rustc --version failed");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// The locked version of one `Cargo.lock` package: read, never asserted from
/// memory.
fn locked_version(package: &str) -> String {
    let lock_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .join("Cargo.lock");
    let lock = fs::read_to_string(&lock_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", lock_path.display()));
    let mut wanted = false;
    for line in lock.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            wanted = false;
        } else if let Some(name) = line.strip_prefix("name = \"") {
            wanted = name.trim_end_matches('"') == package;
        } else if let Some(version) = line.strip_prefix("version = \"")
            && wanted
        {
            return version.trim_end_matches('"').to_owned();
        }
    }
    panic!("package {package:?} is not in {}", lock_path.display());
}

// ---------------------------------------------------------- log parsing ---

/// What the recorded `cargo test` output says actually happened.
#[derive(Debug, Default)]
struct Suite {
    discovered: u64,
    executed: u64,
    passed: u64,
    failed: u64,
    ignored: u64,
    /// `(test name, "pass" | "fail" | "unknown")`, in log order, deduplicated.
    assertions: Vec<(String, &'static str)>,
}

/// Extracts the libtest summaries and the per-test results of the
/// `accept_f27_e_` tests from a recorded `cargo test` output.
fn parse_suite(log: &str) -> Suite {
    let mut suite = Suite::default();
    let mut pending: VecDeque<String> = VecDeque::new();
    for line in log.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("test result:") {
            for (count, kind) in summary_fields(trimmed) {
                match kind {
                    "passed" => suite.passed += count,
                    "failed" => suite.failed += count,
                    "ignored" => suite.ignored += count,
                    _ => {}
                }
            }
            continue;
        }
        if pending.front().is_some() {
            if trimmed == "ok" {
                let name = pending.pop_front().expect("pending test");
                record(&mut suite, name, "pass");
                continue;
            }
            if trimmed == "FAILED" {
                let name = pending.pop_front().expect("pending test");
                record(&mut suite, name, "fail");
                continue;
            }
        }
        let mut cursor = trimmed;
        while let Some(position) = cursor.find("test ") {
            let after = &cursor[position + 5..];
            let Some(separator) = after.find(" ... ") else {
                break;
            };
            let name = after[..separator].to_owned();
            let tail = &after[separator + 5..];
            cursor = tail;
            if !name.starts_with("accept_f27_e_") {
                continue;
            }
            match tail.split_whitespace().next() {
                Some("ok") => record(&mut suite, name, "pass"),
                Some("FAILED") => record(&mut suite, name, "fail"),
                _ => pending.push_back(name),
            }
        }
    }
    suite.assertions.dedup_by(|left, right| left.0 == right.0);
    suite.executed = suite.passed + suite.failed;
    suite.discovered = suite.passed + suite.failed + suite.ignored;
    suite
}

/// `(count, kind)` pairs of one `test result:` summary line.
fn summary_fields(line: &str) -> Vec<(u64, &str)> {
    let mut fields = Vec::new();
    for segment in line["test result:".len()..].split(';') {
        let words: Vec<&str> = segment.split_whitespace().collect();
        for pair in words.windows(2) {
            if let Ok(count) = pair[0].parse::<u64>()
                && matches!(pair[1], "passed" | "failed" | "ignored")
            {
                fields.push((count, pair[1]));
                break;
            }
        }
    }
    fields
}

fn record(suite: &mut Suite, name: String, status: &'static str) {
    if suite.assertions.iter().any(|(seen, _)| *seen == name) {
        return;
    }
    suite.assertions.push((name, status));
}

// ------------------------------------------------------------- artifacts ---

/// One referenced artifact: hashed here with the production SHA-256 the task
/// implements (the validator re-hashes it with `hashlib` independently).
fn artifact(source: &Path, kind: &str, evidence_dir: &Path) -> (String, String, String) {
    let name = source
        .file_name()
        .expect("artifact has a file name")
        .to_string_lossy()
        .into_owned();
    let target = evidence_dir.join(&name);
    if source != target {
        fs::copy(source, &target).unwrap_or_else(|error| {
            panic!("copy {} -> {}: {error}", source.display(), target.display())
        });
    }
    let bytes =
        fs::read(&target).unwrap_or_else(|error| panic!("read {}: {error}", target.display()));
    (name, sha256(&bytes).to_hex(), kind.to_owned())
}

// ------------------------------------------------------------- rendering ---

struct Engine {
    rust: String,
    bevy: String,
    avian: String,
}

fn engine_json(engine: &Engine) -> String {
    format!(
        "{{\"rust\": {}, \"bevy\": {}, \"avian\": {}}}",
        jstr(&engine.rust),
        jstr(&engine.bevy),
        jstr(&engine.avian)
    )
}

fn assertion_array(assertions: &[(String, &'static str)]) -> String {
    let items: Vec<String> = assertions
        .iter()
        .map(|(name, status)| {
            format!(
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\", \
                 \"ammunition-vocabulary.json\"]}}",
                jstr(name)
            )
        })
        .collect();
    items.join(", ")
}

fn artifact_array(artifacts: &[(String, String, String)]) -> String {
    let items: Vec<String> = artifacts
        .iter()
        .map(|(name, digest, kind)| {
            format!(
                "{{\"path\": {}, \"sha256\": {digest:?}, \"kind\": {kind:?}}}",
                jstr(name)
            )
        })
        .collect();
    items.join(", ")
}

fn str_array(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|item| jstr(item)).collect();
    format!("[{}]", quoted.join(", "))
}

/// A JSON string literal: quoted and escaped, so no report field can break
/// out of its string.
fn jstr(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if (control as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// RFC 3339 with whole seconds and `Z`, which `datetime.fromisoformat`
/// accepts after the validator's `Z` -> `+00:00` replacement.
fn iso_utc_now() -> String {
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock is after 1970")
        .as_secs() as i64;
    let (year, month, day, hour, minute, second) = civil_from_unix(epoch);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to a UTC
/// calendar date, because `std` has no date formatting.
fn civil_from_unix(seconds: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (
        if m <= 2 { y + 1 } else { y },
        m as u32,
        d as u32,
        (rest / 3600) as u32,
        ((rest % 3600) / 60) as u32,
        (rest % 60) as u32,
    )
}
