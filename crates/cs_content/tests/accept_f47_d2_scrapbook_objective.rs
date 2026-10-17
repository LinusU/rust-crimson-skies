//! Acceptance stage F47-D.2: what the original scrapbook table's `Objective`
//! position indexes
//! (`specs/F47-scrapbook-records-mementos-and-mission-replay.md`, section
//! `### F47-D`; the follow-up task #774). Shared contracts
//! `docs/contracts/STATE-TRANSACTIONS.md` and `docs/contracts/CLI-EVIDENCE.md`.
//!
//! F47-D left the meaning of the `Objective` position unmeasured. This stage
//! measures and binds it: [`DiscoveredScrapbook::objective_rules`] reads every
//! item's position as the numbered objective (`OBJECTIVE<n>`) of the campaign
//! mission its page names, and reports a `Resolved::Known` rule with the
//! member span as provenance, or an explicit unknown when the position names
//! none.
//!
//! * the **synthetic** tests build a temporary installation holding an
//!   authored table and call the production discovery and the production
//!   binding on it. They run in CI.
//! * the **retail** test (`#[ignore = "requires CS_GAME_DIR"]`) reads the
//!   owner's installation and pins the measured census of the real table. Run
//!   it with `--include-ignored`; without `CS_GAME_DIR` it fails loudly rather
//!   than passing vacuously.
//!
//! Nothing here claims the original *runtime*: whether the engine gates an
//! item's visibility on the objective it names is native behavior this engine
//! has not decoded, and the finding records that instead of guessing it.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use cs_content::catalog::Catalog;
use cs_content::catalog::baseline::retail_baseline;
use cs_content::scrapbook::{
    DiscoveredScrapbook, OBJECTIVE_RULE_CLAIM, ObjectiveCensus, ScrapbookObjectiveRule, audit,
};
use cs_formats::{DIRECTORY_HEADER_BYTES, FLAG_DIRECTORY, RECORD_BYTES};
use cs_types::content::{
    CatalogElement, ContentId, ContentKind, NormalizeState, Origin, Readiness, Resolved,
};
use cs_types::evidence::ClaimStatus;
use cs_types::install::ParseState;

// ------------------------------------------------------------- fixtures ---

/// A temporary installation directory, removed when the test ends.
struct TempInstall(PathBuf);

impl TempInstall {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f47-d2-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("the fixture directory is created");
        Self(root)
    }

    fn write(&self, relative: &str, bytes: &[u8]) {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().expect("a parent directory"))
            .expect("the fixture directory is created");
        fs::write(&path, bytes).expect("the fixture file is written");
    }
}

impl Drop for TempInstall {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// One authored file entry: its name and the bytes stored in the container.
struct Entry {
    name: String,
    stored: Vec<u8>,
}

impl Entry {
    fn plain(name: &str, bytes: &[u8]) -> Self {
        Self {
            name: name.to_owned(),
            stored: bytes.to_vec(),
        }
    }
}

fn rof_names(names: &[&str]) -> Vec<u8> {
    let mut table = Vec::new();
    for name in names {
        table.extend_from_slice(name.as_bytes());
        table.push(0);
    }
    table
}

/// One directory block: header, 24-byte records, name table.
fn block(records: &[[u32; 6]], names: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(records.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(names.len() as u32).to_le_bytes());
    for record in records {
        for word in record {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
    }
    bytes.extend_from_slice(names);
    bytes
}

/// `[directory: entries…][payloads…]`, one directory deep: the shape of the
/// retail archive's `ASSETS/` directory.
fn rof_container(directory: &str, entries: &[Entry]) -> Vec<u8> {
    let root_names = rof_names(&[directory]);
    let root_len = DIRECTORY_HEADER_BYTES + RECORD_BYTES + root_names.len();
    let entry_names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
    let sub_names = rof_names(&entry_names);
    let sub_len = DIRECTORY_HEADER_BYTES + entries.len() * RECORD_BYTES + sub_names.len();

    let root = block(
        &[[
            root_len as u32,
            0,
            0,
            FLAG_DIRECTORY,
            directory.len() as u32 + 1,
            1,
        ]],
        &root_names,
    );
    let mut cursor = (root_len + sub_len) as u32;
    let mut records = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let stored = entry.stored.len() as u32;
        records.push([
            cursor,
            stored,
            stored,
            0,
            entry.name.len() as u32 + 1,
            10 + index as u32,
        ]);
        cursor += stored;
    }
    let mut bytes = root;
    bytes.extend_from_slice(&block(&records, &sub_names));
    for entry in entries {
        bytes.extend_from_slice(&entry.stored);
    }
    assert_eq!(bytes.len(), cursor as usize, "every payload placed once");
    bytes
}

/// One authored `Mission_Spread_Item` record line: the sixteen documented
/// fields, the first field whatever the caller wants to spell there.
fn record_line(key: &str, objective: &str, image: &str) -> String {
    format!(
        "{key}={objective},0,{image},P0,10,20,2,0,0,80,\"0,0,0,0\",A,0,0,IDS_TITLE,IDS_TEXT\r\n"
    )
}

/// The authored table: a header, the field-list comment and `lines`.
fn table(lines: &[String]) -> Vec<u8> {
    let mut csv = Vec::new();
    csv.extend_from_slice(b"[SCRAPBOOK]\r\n");
    csv.extend_from_slice(
        b";Mission_Spread_Item=Objective,ResourceID,ImageName,ImageType,X,Y,\
                           Alpha,Width,Height,DrawOrder,\"Left,Top,Right,Bottom\",Zoom,ZoomX,\
                           ZoomY,TitleResID,TextResID\r\n",
    );
    for line in lines {
        csv.extend_from_slice(line.as_bytes());
    }
    csv
}

/// An installation holding `member` as the scrapbook table plus the named
/// artwork members under the container's artwork directory.
fn install(label: &str, member: &[u8], artwork: &[&str]) -> TempInstall {
    let temp = TempInstall::new(label);
    let mut entries = vec![Entry::plain("SCRAPBOOK.CSV", member)];
    for name in artwork {
        let spelling = if name.contains('/') {
            (*name).to_owned()
        } else {
            format!("GRAPHICS/SCRAPBOOK/{name}")
        };
        entries.push(Entry::plain(&spelling, b"authored picture bytes"));
    }
    temp.write(
        "GOSDATA/ASSETS/crimson.rof",
        &rof_container("ASSETS", &entries),
    );
    temp
}

/// A progression holding exactly the missions named, in the order given.
fn progression(missions: &[&str]) -> Catalog {
    let mut catalog = Catalog::new();
    for key in missions {
        catalog
            .insert(CatalogElement {
                kind: ContentKind::Mission,
                id: ContentId::from_source(ContentKind::Mission, key).expect("a mission id"),
                display_name: None,
                origin: Origin::SyntheticFixture,
                dependencies: Vec::new(),
                parse_state: ParseState::Parsed,
                normalize_state: NormalizeState::Normalized,
                runtime_consumers: Vec::new(),
                readiness: Readiness::Ready,
                unsupported_reasons: Vec::new(),
                fingerprint: None,
            })
            .expect("the mission row inserts");
    }
    catalog
}

fn id_key(key: &str) -> String {
    cs_content::catalog::baseline::install_file_key(key)
}

/// The binding of one item, looked up by its entry key.
fn rule_of<'a>(census: &'a ObjectiveCensus, key: &str) -> &'a Resolved<ScrapbookObjectiveRule> {
    let wanted = id_key(key);
    census
        .rules
        .iter()
        .find(|rule| rule.item.as_deref() == Some(wanted.as_str()))
        .map(|rule| &rule.rule)
        .unwrap_or_else(|| panic!("the census holds no binding for {key}"))
}

/// A tiny RFC-8259 check, so a malformed audit artifact fails the test (the
/// workspace has no JSON dependency).
fn assert_parses_as_json(text: &str) {
    let bytes = text.as_bytes();
    let mut cursor = 0;
    if let Err(error) = json_value(bytes, &mut cursor) {
        panic!("the artifact is not valid JSON at byte {cursor}: {error}; artifact: {text}");
    }
    json_space(bytes, &mut cursor);
    assert_eq!(
        cursor,
        bytes.len(),
        "the artifact has trailing bytes; artifact: {text}"
    );
}

fn json_space(bytes: &[u8], cursor: &mut usize) {
    while matches!(bytes.get(*cursor), Some(b' ' | b'\t' | b'\n' | b'\r')) {
        *cursor += 1;
    }
}

fn json_value(bytes: &[u8], cursor: &mut usize) -> Result<(), String> {
    json_space(bytes, cursor);
    match bytes.get(*cursor) {
        Some(b'{') => json_object(bytes, cursor),
        Some(b'[') => json_array(bytes, cursor),
        Some(b'"') => json_string(bytes, cursor),
        Some(b't') => json_literal(bytes, cursor, "true"),
        Some(b'f') => json_literal(bytes, cursor, "false"),
        Some(b'n') => json_literal(bytes, cursor, "null"),
        Some(byte) if byte.is_ascii_digit() || *byte == b'-' => json_number(bytes, cursor),
        Some(byte) => Err(format!("unexpected byte {byte:#04x}")),
        None => Err("the value is missing".to_owned()),
    }
}

fn json_object(bytes: &[u8], cursor: &mut usize) -> Result<(), String> {
    *cursor += 1;
    json_space(bytes, cursor);
    if bytes.get(*cursor) == Some(&b'}') {
        *cursor += 1;
        return Ok(());
    }
    loop {
        json_space(bytes, cursor);
        json_string(bytes, cursor)?;
        json_space(bytes, cursor);
        if bytes.get(*cursor) != Some(&b':') {
            return Err("a member needs ':'".to_owned());
        }
        *cursor += 1;
        json_value(bytes, cursor)?;
        json_space(bytes, cursor);
        match bytes.get(*cursor) {
            Some(b',') => *cursor += 1,
            Some(b'}') => {
                *cursor += 1;
                return Ok(());
            }
            _ => return Err("a member needs ',' or the object needs '}'".to_owned()),
        }
    }
}

fn json_array(bytes: &[u8], cursor: &mut usize) -> Result<(), String> {
    *cursor += 1;
    json_space(bytes, cursor);
    if bytes.get(*cursor) == Some(&b']') {
        *cursor += 1;
        return Ok(());
    }
    loop {
        json_value(bytes, cursor)?;
        json_space(bytes, cursor);
        match bytes.get(*cursor) {
            Some(b',') => *cursor += 1,
            Some(b']') => {
                *cursor += 1;
                return Ok(());
            }
            _ => return Err("an element needs ',' or the array needs ']'".to_owned()),
        }
    }
}

fn json_string(bytes: &[u8], cursor: &mut usize) -> Result<(), String> {
    if bytes.get(*cursor) != Some(&b'"') {
        return Err("a string must open with '\"'".to_owned());
    }
    *cursor += 1;
    loop {
        let byte = *bytes
            .get(*cursor)
            .ok_or_else(|| "the string is not closed".to_owned())?;
        match byte {
            b'"' => {
                *cursor += 1;
                return Ok(());
            }
            b'\\' => {
                *cursor += 1;
                let escape = *bytes
                    .get(*cursor)
                    .ok_or_else(|| "the escape is cut short".to_owned())?;
                *cursor += 1;
                match escape {
                    b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => {}
                    b'u' => {
                        for _ in 0..4 {
                            let digit = *bytes
                                .get(*cursor)
                                .ok_or_else(|| "the escape is cut short".to_owned())?;
                            *cursor += 1;
                            if !digit.is_ascii_hexdigit() {
                                return Err(format!("escape byte {digit:#04x} is not hex"));
                            }
                        }
                    }
                    other => return Err(format!("unknown escape {other:#04x}")),
                }
            }
            control if control < 0x20 => {
                return Err(format!("raw control byte {control:#04x} inside a string"));
            }
            _ => *cursor += 1,
        }
    }
}

fn json_number(bytes: &[u8], cursor: &mut usize) -> Result<(), String> {
    if bytes.get(*cursor) == Some(&b'-') {
        *cursor += 1;
    }
    let digits = |bytes: &[u8], cursor: &mut usize| {
        let start = *cursor;
        while bytes.get(*cursor).is_some_and(u8::is_ascii_digit) {
            *cursor += 1;
        }
        *cursor > start
    };
    if !digits(bytes, cursor) {
        return Err("a number needs an integer part".to_owned());
    }
    if bytes.get(*cursor) == Some(&b'.') {
        *cursor += 1;
        if !digits(bytes, cursor) {
            return Err("a fraction needs digits after '.'".to_owned());
        }
    }
    if matches!(bytes.get(*cursor), Some(b'e' | b'E')) {
        *cursor += 1;
        if matches!(bytes.get(*cursor), Some(b'+' | b'-')) {
            *cursor += 1;
        }
        if !digits(bytes, cursor) {
            return Err("an exponent needs digits".to_owned());
        }
    }
    Ok(())
}

fn json_literal(bytes: &[u8], cursor: &mut usize, literal: &str) -> Result<(), String> {
    let word = literal.as_bytes();
    if bytes.get(*cursor..*cursor + word.len()) == Some(word) {
        *cursor += word.len();
        Ok(())
    } else {
        Err(format!("expected {literal}"))
    }
}

// ----------------------------------------------------------------- tests ---

/// A `Snap_<page>_<n>` item's `Objective` position is the numbered objective of
/// the campaign mission its page names, bound with the member span as
/// provenance; a position that names no objective stays an explicit unknown.
#[test]
fn accept_f47_d2_the_objective_position_binds_a_snapshot_to_its_missions_objective() {
    let lines = vec![
        record_line("1_1_1", "18", "Snap_1_18"),
        record_line("1_1_2", "0", "SB_00_00_news1"),
        record_line("1_1_3", "-11", "SB_01_01_swannote"),
        record_line("2_1_1", "20", "Snap_2_20"),
        // The name and the position disagree: counted, never silently merged.
        record_line("2_1_2", "7", "Snap_2_19"),
        // A page the progression holds no mission for.
        record_line("9_1_1", "3", "SB_09_01_crawpic"),
    ];
    let member = table(&lines);
    let temp = install(
        "objective",
        &member,
        &[
            "Snap_1_18.PNG",
            "SB_00_00_news1.PNG",
            "SB_01_01_swannote.PNG",
            "Snap_2_20.PNG",
            "Snap_2_19.PNG",
            "SB_09_01_crawpic.PNG",
        ],
    );
    let discovered = DiscoveredScrapbook::discover(&temp.0).expect("the authored table reads");
    let progression = progression(&["ch1-m01", "ch1-m02"]);

    let census = discovered.objective_rules(&progression);
    assert_eq!(census.items, 6, "every item is examined");
    assert_eq!(census.numbered, 4, "the four positive positions");
    assert_eq!(census.special, 2, "0 and a negative name no objective");
    assert_eq!(census.unparsed, 0);
    assert_eq!(census.bound, 3, "three numbered items reach a mission row");
    assert_eq!(
        census.page_without_mission,
        vec![9],
        "a numbered item whose page names no mission is a named gap, not a guessed rule"
    );
    assert_eq!(census.snapshot_items, 3, "the three Snap_ items");
    assert_eq!(
        census.snapshot_agree, 2,
        "two Snap_ names spell exactly the position they carry"
    );
    assert_eq!(
        census.snapshot_disagree,
        vec![id_key("2_1_2")],
        "the one Snap_ name that disagrees is listed by id, never merged"
    );
    assert_eq!(
        census.rules.len(),
        6,
        "one binding per item, in member order"
    );

    // The measured rule: page 1 is the first campaign mission, objective 18.
    let Resolved::Known(known) = rule_of(&census, "1_1_1") else {
        panic!("the snapshot binds to a known rule");
    };
    assert_eq!(
        known.value,
        ScrapbookObjectiveRule {
            mission: ContentId::from_source(ContentKind::Mission, "ch1-m01").expect("mission"),
            objective: 18,
        }
    );
    assert_eq!(
        known.provenance.claim_id.as_str(),
        OBJECTIVE_RULE_CLAIM,
        "the binding cites its own claim"
    );
    assert_eq!(known.provenance.class, ClaimStatus::ObservedTool);
    let span = known
        .provenance
        .source
        .as_ref()
        .expect("the binding cites the table member's own span");
    assert_eq!(span.container_path(), "GOSDATA/ASSETS/crimson.rof");
    assert_eq!(span.member_key(), Some("ASSETS/SCRAPBOOK.CSV"));
    assert_eq!(
        span.member_sha256().map(|hash| hash.to_hex()),
        Some(discovered.member_sha256.clone()),
        "the span's digest is the table member's decoded digest"
    );

    // Page 2 is the second campaign mission.
    let Resolved::Known(known) = rule_of(&census, "2_1_1") else {
        panic!("the second page binds too");
    };
    assert_eq!(known.value.objective, 20);
    assert_eq!(known.value.mission.key(), "ch1-m02");

    // A position of 0, a negative position and an unbound page are explicit
    // unknowns, never a zero or a guessed mission.
    for key in ["1_1_2", "1_1_3", "9_1_1"] {
        assert!(
            matches!(rule_of(&census, key), Resolved::Unknown { .. }),
            "{key} must stay an explicit unknown"
        );
    }

    // The audit carries the same census and serializes it as JSON.
    let declared = cs_content::scrapbook::ScrapbookCatalog::new(Vec::new()).expect("empty");
    let report = audit(&discovered, &declared, &progression);
    assert_eq!(report.objective, census);
    assert!(
        !report.is_complete(),
        "a page without a mission keeps the audit incomplete"
    );
    let json = report.json();
    assert_parses_as_json(&json);
    for needle in [
        "\"objective\": {",
        "\"numbered\": 4",
        "\"special\": 2",
        "\"bound\": 3",
        "\"page_without_mission\": [9]",
        "\"snapshot_items\": 3",
        "\"snapshot_agree\": 2",
    ] {
        assert!(
            json.contains(needle),
            "the artifact is missing {needle:?}:\n{json}"
        );
    }
    assert!(
        json.contains(&format!("\"snapshot_disagree\": [\"{}\"]", id_key("2_1_2"))),
        "the artifact must list the disagreeing snapshot by id:\n{json}"
    );
}

/// A position the schema accepts but that is not a whole objective number at
/// all (an out-of-range integer) and a page the progression holds no mission
/// for are both explicit unknowns, never a zero or a guessed mission.
#[test]
fn accept_f47_d2_a_position_with_no_objective_and_a_page_with_no_mission_are_unknowns() {
    let lines = vec![
        // A whole number to the schema, but larger than any objective id.
        record_line("1_1_1", "99999999999999999999999999", "SB_01_01_coin"),
        record_line("1_1_2", "3", "SB_01_01_news3"),
        // A page beyond the missions the progression holds.
        record_line("5_1_1", "4", "SB_05_01_tchest"),
    ];
    let member = table(&lines);
    let temp = install("unknown", &member, &["SB_01_01_coin.PNG"]);
    let discovered = DiscoveredScrapbook::discover(&temp.0).expect("the authored table reads");
    let only_one = progression(&["ch1-m01"]);

    let census = discovered.objective_rules(&only_one);
    assert_eq!(census.items, 3);
    assert_eq!(census.unparsed, 1, "the out-of-range position");
    assert_eq!(census.numbered, 2);
    assert_eq!(census.bound, 1, "only page 1 names a mission");
    assert_eq!(census.special, 0);
    assert_eq!(
        census.page_without_mission,
        vec![5],
        "a numbered item whose page names no mission is a named gap"
    );
    assert!(
        matches!(rule_of(&census, "1_1_1"), Resolved::Unknown { .. }),
        "an out-of-range position stays unknown"
    );
    assert!(
        matches!(rule_of(&census, "5_1_1"), Resolved::Unknown { .. }),
        "a page with no mission stays unknown"
    );

    let declared = cs_content::scrapbook::ScrapbookCatalog::new(Vec::new()).expect("empty");
    let report = audit(&discovered, &declared, &only_one);
    assert!(
        !report.is_complete(),
        "a page the progression cannot answer keeps the audit incomplete"
    );
    // The same table with a mission for page 5 binds it: the gap is the
    // progression's, not the item's.
    let with_page_five = DiscoveredScrapbook::discover(&temp.0).expect("the table reads");
    let five = progression(&["ch1-m01", "ch2-m02", "ch3-m03", "ch4-m04", "ch5-m05"]);
    let census = with_page_five.objective_rules(&five);
    assert_eq!(census.bound, 2);
    assert!(
        census.page_without_mission.is_empty(),
        "every numbered page now names a mission"
    );
}

/// The owner's installation, read through production discovery: the measured
/// census of the real table's `Objective` position.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f47_d2_retail_the_installation_objective_census_is_measured() {
    let game_dir = std::path::PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR is set for a retail acceptance test"),
    );
    let discovered = DiscoveredScrapbook::discover(&game_dir)
        .expect("the installation's scrapbook table reads end to end");
    let baseline = retail_baseline(&game_dir).expect("the production baseline reads");

    let census = discovered.objective_rules(&baseline.catalog);
    assert_eq!(census.items, 461, "every measured record");
    assert_eq!(
        census.numbered, 447,
        "461 minus the 10 initial-page rows and the 4 negative positions"
    );
    assert_eq!(census.special, 14, "page 0's ten rows and four negatives");
    assert_eq!(census.unparsed, 0, "every position is a whole number");
    assert_eq!(
        census.bound, 447,
        "every numbered position's page is one of the 24 campaign missions"
    );
    assert!(
        census.page_without_mission.is_empty(),
        "every numbered page names a mission"
    );
    assert_eq!(census.snapshot_items, 167, "the runtime-snapshot rows");
    assert_eq!(
        census.snapshot_agree, 165,
        "all but two snapshot names spell exactly their position"
    );
    assert_eq!(
        census.snapshot_disagree.len(),
        2,
        "the two names that disagree are listed, not merged"
    );
    assert_eq!(census.rules.len(), 461, "one binding per item");

    // The first Hawaii page's first snapshot binds to the campaign's first
    // mission, at the objective its own name spells.
    let first = census
        .rules
        .iter()
        .find(|rule| rule.item.as_deref() == Some(id_key("1_2_6").as_str()))
        .expect("the retail table's 1_2_6 row");
    let Resolved::Known(known) = &first.rule else {
        panic!("the Snap_1_18 row binds to a known rule");
    };
    assert_eq!(known.value.mission.key(), "ch1-m01");
    assert_eq!(known.value.objective, 18);

    let declared = cs_content::scrapbook::ScrapbookCatalog::new(Vec::new()).expect("empty");
    let report = audit(&discovered, &declared, &baseline.catalog);
    assert_eq!(report.objective, census);
    let json = report.json();
    assert_parses_as_json(&json);
    let expected: BTreeMap<&str, &str> = [
        ("\"numbered\": 447", "the numbered positions"),
        ("\"special\": 14", "the initial page and the negatives"),
        ("\"bound\": 447", "the bound rules"),
        ("\"page_without_mission\": []", "no page is unattributed"),
        ("\"snapshot_items\": 167", "the snapshot rows"),
        ("\"snapshot_agree\": 165", "the agreeing names"),
    ]
    .into_iter()
    .collect();
    for (needle, what) in expected {
        assert!(
            json.contains(needle),
            "the artifact is missing {what} ({needle:?}):\n{json}"
        );
    }
}
