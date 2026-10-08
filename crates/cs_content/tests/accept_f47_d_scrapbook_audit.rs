//! Acceptance stage F47-D: the retail scrapbook audit
//! (`specs/F47-scrapbook-records-mementos-and-mission-replay.md`, section
//! `### F47-D`), shared contract `docs/contracts/STATE-TRANSACTIONS.md` and
//! the evidence contract `docs/contracts/CLI-EVIDENCE.md`.
//!
//! The stage's minimum scenario is **AC04 — audit every discovered scrapbook
//! page, memento and replay link against original progression**. The audit is
//! [`cs_content::scrapbook::audit`], over the discovery
//! [`cs_content::scrapbook::DiscoveredScrapbook::discover`] reads out of the
//! installation's own `ASSETS/SCRAPBOOK.CSV` with the production ROF mount
//! and the production keyed-list reader.
//!
//! * the **synthetic** tests build a temporary installation holding an
//!   authored table and authored artwork members, then call the production
//!   discovery and the production audit on it: grouping, artwork joining,
//!   named gaps, the refusal of a table that cannot be read, and every arm of
//!   the audit (matched, undeclared, fabricated, unbacked rule, missing
//!   subject, missing replay mission, memento). They run in CI.
//! * the **retail** test (`#[ignore = "requires CS_GAME_DIR"]`) reads the
//!   owner's installation, pins the measured shape of the real table and
//!   audits it against the real progression. Run it with
//!   `--include-ignored`; without `CS_GAME_DIR` it fails loudly rather than
//!   passing vacuously.
//!
//! Nothing here claims the original's *semantics*: the table documents no
//! unlock field, no mission field and no memento record, and the audit says
//! so instead of inventing any of the three.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use cs_assets::install::sha256;
use cs_content::catalog::Catalog;
use cs_content::catalog::baseline::{install_file_key, retail_baseline};
use cs_content::scrapbook::{
    DiscoveredScrapbook, EntryKind, EntryVisibility, ReplayLink, ScrapbookCatalog, ScrapbookEntry,
    ScrapbookSourceError, Unlock, UnlockFact, UnlockFactKind, audit,
};
use cs_formats::{DIRECTORY_HEADER_BYTES, FLAG_DIRECTORY, RECORD_BYTES};
use cs_types::content::{
    CatalogElement, ContentId, ContentKind, Known, NormalizeState, Origin, Provenance, Readiness,
    Resolved,
};
use cs_types::evidence::ClaimId;
use cs_types::install::ParseState;

// ------------------------------------------------------------- fixtures ---

/// A temporary installation directory, removed when the test ends.
struct TempInstall(PathBuf);

impl TempInstall {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f47-d-{label}-{}-{}",
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
/// fields, the numeric `Objective` first.
fn record(key: &str, objective: i64, image: &str) -> String {
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

/// An installation holding `member` as the scrapbook table plus `artwork`
/// members under the container's artwork directory.
fn install(label: &str, member: &[u8], artwork: &[&str]) -> TempInstall {
    let temp = TempInstall::new(label);
    let mut entries = vec![Entry::plain("SCRAPBOOK.CSV", member)];
    for name in artwork {
        // A name that already carries a directory is stored where it says, so
        // a picture *outside* the artwork directory can be authored.
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

// --------------------------------------------------------- id builders ---

fn id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("content id")
}

fn mission(key: &str) -> ContentId {
    id(ContentKind::Mission, key)
}

fn item(key: &str) -> ContentId {
    id(ContentKind::ScrapbookItem, key)
}

fn text(key: &str) -> ContentId {
    id(ContentKind::StringResource, key)
}

/// A `Known` unlock rule with designed provenance.
fn known(unlock: Unlock) -> Resolved<Unlock> {
    Resolved::Known(Known::new(
        unlock,
        Provenance::designed(ClaimId::new("f47d.test-rule").expect("claim")),
    ))
}

/// An explicit unknown rule: it never unlocks.
fn unknown() -> Resolved<Unlock> {
    Resolved::unknown(
        ClaimId::new("f47d.test-unknown").expect("claim"),
        "the original table documents no unlock field",
    )
    .expect("a reason")
}

/// One declared entry: a hidden page keyed by `key`.
fn entry(key: &str, unlock: Resolved<Unlock>) -> ScrapbookEntry {
    ScrapbookEntry {
        id: item(key),
        kind: EntryKind::Page,
        title: text(&format!("{key}-title")),
        image: None,
        unlock,
        visibility: EntryVisibility::HiddenUntilUnlocked,
        replay: None,
    }
}

/// A progression holding exactly the missions named.
fn progression(missions: &[&str]) -> Catalog {
    let mut catalog = Catalog::new();
    for key in missions {
        let content = mission(key);
        catalog
            .insert(CatalogElement {
                kind: ContentKind::Mission,
                id: content,
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

// -------------------------------------------------------------- JSON ---

/// A strict RFC-8259 parse of the artifact, so a bare word where a string
/// belongs fails the test (the workspace has no JSON dependency).
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
        "the artifact has {} trailing bytes after its value; artifact: {text}",
        bytes.len() - cursor
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
    *cursor += 1; // `{`
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
    *cursor += 1; // `[`
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
                                return Err(format!("escape byte {digit:#04x} is not a hex digit"));
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

/// Every authored record is grouped into the page its key spells, in the
/// member's own order, joined to the artwork the container really holds, and
/// everything that cannot be paged is a named gap — never dropped.
#[test]
fn accept_f47_d_every_discovered_record_is_grouped_into_the_pages_its_keys_spell() {
    let lines = vec![
        record("0_1_1", 1, "ART_A"),
        // The same key with the same fields: one item, one counted repeat.
        record("0_1_1", 1, "ART_A"),
        record("0_1_2", 2, "GONE"),
        record("1_1_1", 3, "ART_B"),
        // A picture stored outside the artwork directory: found, and counted.
        record("1_1_2", 4, "STRAY"),
        // A key that is not `<page>_<spread>_<slot>`: counted, not guessed.
        record("odd_key", 5, "ART_C"),
        // A layout record the scrapbook schema does not cover.
        "BTNSHAPE=B,art.png,1,2,3,0,IDS_LABEL,4,5,,,,,6,1\r\n".to_owned(),
    ];
    let member = table(&lines);
    let temp = install(
        "pages",
        &member,
        &["ART_A.PNG", "ART_B.PNG", "OTHER/STRAY.PNG"],
    );
    let discovered = DiscoveredScrapbook::discover(&temp.0).expect("the authored table reads");

    assert_eq!(discovered.records, 5, "five keys the schema covers");
    assert_eq!(discovered.page_count(), 2, "pages 0 and 1");
    assert_eq!(discovered.paged_items(), 4, "every paged item");
    let pages: Vec<u32> = discovered.pages.iter().map(|page| page.page).collect();
    assert_eq!(pages, vec![0, 1], "pages are ordered by their own number");
    let keys: Vec<&str> = discovered.pages[0]
        .items
        .iter()
        .map(|item| item.key.as_str())
        .collect();
    assert_eq!(
        keys,
        vec!["0_1_1", "0_1_2"],
        "the page keeps the member's own order, one row per key"
    );
    assert_eq!(discovered.pages[0].items[0].spread, 1);
    assert_eq!(discovered.pages[0].items[0].slot, 1);
    assert_eq!(
        discovered.pages[0].items[0].objective,
        Some(1),
        "the Objective field is converted, never read as a rule"
    );
    assert_eq!(
        discovered.pages[0].items[0].artwork,
        vec!["ASSETS/GRAPHICS/SCRAPBOOK/ART_A.PNG".to_owned()],
        "the picture is the container's own member, by name"
    );
    assert_eq!(
        discovered.pages[0].items[0].capture.as_deref(),
        Some("ASSETS/GRAPHICS/SCRAPBOOK/ART_A.PNG"),
        "the capture prefers the extension production code can decode"
    );
    assert_eq!(
        discovered.pages[0].items[1].artwork,
        Vec::<String>::new(),
        "an image the installation does not hold has no artwork"
    );
    assert_eq!(
        discovered.pages[1].items[1].artwork,
        vec!["ASSETS/OTHER/STRAY.PNG".to_owned()],
        "a picture stored outside the artwork directory is still found, and named as such"
    );
    assert_eq!(discovered.items_with_artwork(), 3);
    assert_eq!(
        discovered.pages_without_artwork(),
        Vec::<u32>::new(),
        "both pages hold at least one picture"
    );

    assert_eq!(
        gaps_of_report(&discovered.gaps),
        vec![
            ("ambiguous_entry_key".to_owned(), 0),
            ("artwork_outside_artwork_directory".to_owned(), 1),
            ("duplicate_entry_key".to_owned(), 1),
            ("entry_key_not_page_structured".to_owned(), 1),
            ("entry_not_a_scrapbook_item".to_owned(), 1),
            ("item_without_artwork".to_owned(), 1),
            ("entry_key_not_utf8".to_owned(), 0),
            ("artwork_undecodable_format".to_owned(), 0),
        ],
        "every refusal is named and counted"
    );
    assert_eq!(
        discovered.gaps.len(),
        5,
        "five named gaps, and no unnamed one"
    );
    assert_eq!(
        discovered.member_sha256,
        sha256(&member).to_hex(),
        "the member the records were read from is fingerprinted"
    );
    assert_eq!(discovered.install_sha256.len(), 64);
    assert_eq!(discovered.content_sha256.len(), 64);

    let json = discovered.json();
    assert_parses_as_json(&json);
    for needle in [
        "\"page_count\": 2",
        "\"records\": 5",
        "\"paged_items\": 4",
        "\"items_with_artwork\": 3",
        "\"item_without_artwork\": 1",
        "\"pages\": [",
    ] {
        assert!(
            json.contains(needle),
            "the artifact is missing {needle:?}:\n{json}"
        );
    }
}

/// Gaps with a count of zero are absent from the map; the helper keeps the
/// assertion above readable by zero-filling the codes it expects.
fn gaps_of_report(gaps: &std::collections::BTreeMap<String, u32>) -> Vec<(String, u32)> {
    let codes = [
        "ambiguous_entry_key",
        "artwork_outside_artwork_directory",
        "duplicate_entry_key",
        "entry_key_not_page_structured",
        "entry_not_a_scrapbook_item",
        "item_without_artwork",
        "entry_key_not_utf8",
        "artwork_undecodable_format",
    ];
    codes
        .iter()
        .map(|code| ((*code).to_owned(), gaps.get(*code).copied().unwrap_or(0)))
        .collect()
}

/// The audit reports every arm AC04 needs: an original item no declaration
/// names, a declaration the original does not make, a rule the original
/// cannot back, a subject and a mission the progression does not hold, and a
/// declared memento — while a table the container holds no record of is an
/// error, never an empty reading.
#[test]
fn accept_f47_d_the_audit_reports_every_declaration_against_the_original() {
    let lines = vec![record("0_1_1", 1, "ART_A"), record("1_1_1", 3, "ART_B")];
    let member = table(&lines);
    let temp = install("audit", &member, &["ART_A.PNG", "ART_B.PNG"]);
    let discovered = DiscoveredScrapbook::discover(&temp.0).expect("the table reads");

    // The declared catalog: two entries the original declares, three it does
    // not, two `Known` rules the table cannot back, one explicit unknown, a
    // replay link onto a mission the progression holds and one onto a mission
    // it does not, and a memento.
    let mut matched = entry(
        &install_file_key("0_1_1"),
        known(Unlock::Fact(UnlockFact {
            kind: UnlockFactKind::MissionSucceeded,
            subject: mission("ch1-m09"),
        })),
    );
    matched.replay = Some(ReplayLink {
        mission: mission("ch1-m01"),
        variant: None,
    });
    let mut linked = entry(&install_file_key("1_1_1"), unknown());
    linked.replay = Some(ReplayLink {
        mission: mission("never"),
        variant: None,
    });
    let mut memento = entry(
        "keepsake",
        known(Unlock::Fact(UnlockFact {
            kind: UnlockFactKind::MissionSucceeded,
            subject: mission("ch1-m01"),
        })),
    );
    memento.kind = EntryKind::Memento;
    let fabric = entry(
        "fabricated",
        known(Unlock::Fact(UnlockFact {
            kind: UnlockFactKind::StuntCompleted,
            subject: id(ContentKind::Stunt, "never-flew"),
        })),
    );
    let declared = ScrapbookCatalog::new(vec![matched, linked, memento, fabric])
        .expect("the declared catalog validates");
    let progression = progression(&["ch1-m01"]);

    let report = audit(&discovered, &declared, &progression);
    assert_eq!(report.declared, 4);
    assert_eq!(report.declared_matched, 2, "the two original keys");
    assert_eq!(
        report
            .declared_without_original
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["scrapbook_item/keepsake", "scrapbook_item/fabricated"],
        "a fabricated entry is a defect of the declaration, listed by id"
    );
    assert_eq!(
        report.undeclared_items, 0,
        "both original items are declared"
    );
    assert_eq!(report.unlock_known, 3);
    assert_eq!(report.unlock_unknown, 1, "the explicit unknown");
    assert_eq!(
        report.unlock_unbacked, 3,
        "the table documents no unlock field, so no Known rule is original-backed"
    );
    assert_eq!(
        report.unlock_subjects_missing,
        vec!["mission/ch1-m09".to_owned(), "stunt/never-flew".to_owned(),],
        "a rule naming content the installation does not hold is reported"
    );
    assert_eq!(report.replay_links, 2);
    assert_eq!(
        report.replay_missions_missing,
        vec!["mission/never".to_owned()],
        "only the link the progression cannot answer is missing"
    );
    assert_eq!(report.declared_mementos, 1);
    assert_eq!(report.progression_missions, 1);
    assert_eq!(report.memento_named_images, 0, "no MS_P_ image here");
    assert!(
        !report.is_complete(),
        "an audit with gaps must not report itself complete"
    );
    let json = report.json();
    assert_parses_as_json(&json);
    for needle in [
        "\"original_unlock_fields\": 0",
        "\"original_replay_fields\": 0",
        "\"original_memento_records\": 0",
        "\"unlock_unbacked\": 3",
        "\"backing\": {",
    ] {
        assert!(
            json.contains(needle),
            "the artifact is missing {needle:?}:\n{json}"
        );
    }

    // A declaration of nothing audits as nothing declared: every original
    // item is undeclared, and the report is not complete.
    let empty = audit(
        &discovered,
        &ScrapbookCatalog::new(Vec::new()).expect("empty"),
        &progression,
    );
    assert_eq!(empty.declared, 0);
    assert_eq!(empty.undeclared_items, 2);
    assert!(!empty.is_complete());
}

/// A table that cannot be read is an error that names its source: a missing
/// archive, a missing member and a member holding no scrapbook record all
/// refuse, so an unreadable table can never look like an empty scrapbook.
#[test]
fn accept_f47_d_a_table_that_cannot_be_read_is_an_error_not_an_empty_reading() {
    let missing_container = TempInstall::new("no-container");
    let error = DiscoveredScrapbook::discover(&missing_container.0)
        .expect_err("an installation without the archive cannot be audited");
    assert_eq!(error, ScrapbookSourceError::MissingContainer);
    assert!(
        error.to_string().contains("GOSDATA/ASSETS/crimson.rof"),
        "the error names the archive: {error}"
    );

    let container_only = TempInstall::new("container-only");
    container_only.write(
        "GOSDATA/ASSETS/crimson.rof",
        &rof_container(
            "ASSETS",
            &[Entry::plain(
                "GRAPHICS/SCRAPBOOK/ART_A.PNG",
                b"authored picture bytes",
            )],
        ),
    );
    let error = DiscoveredScrapbook::discover(&container_only.0)
        .expect_err("an archive without the table cannot be audited");
    assert_eq!(error, ScrapbookSourceError::MissingMember);
    assert!(
        error.to_string().contains("ASSETS/SCRAPBOOK.CSV"),
        "the error names the member: {error}"
    );

    let not_a_table = install(
        "not-a-table",
        b"[SCRAPBOOK]\r\na line that is not a record\r\n",
        &[],
    );
    let error = DiscoveredScrapbook::discover(&not_a_table.0)
        .expect_err("a member with no scrapbook record cannot be audited");
    assert!(
        matches!(error, ScrapbookSourceError::Document { .. }),
        "{error}"
    );
    assert!(
        error.to_string().contains("Mission_Spread_Item"),
        "the error says what the member lacks: {error}"
    );
}

/// The owner's installation, read through production discovery: the measured
/// shape of the real table, and the audit of it against the real progression.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f47_d_retail_the_installation_scrapbook_is_discovered_and_audited() {
    let game_dir = std::path::PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR is set for a retail acceptance test"),
    );
    let discovered = DiscoveredScrapbook::discover(&game_dir)
        .expect("the installation's scrapbook table reads end to end");

    assert_eq!(discovered.member_sha256, EXPECTED_MEMBER_SHA256);
    assert_eq!(discovered.records, 461, "the measured record count");
    assert_eq!(
        discovered.page_count(),
        25,
        "pages 0..=24, one run per first key component"
    );
    assert_eq!(discovered.paged_items(), 461, "every record is paged");
    let pages: Vec<u32> = discovered.pages.iter().map(|page| page.page).collect();
    assert_eq!(
        pages,
        (0..=24).collect::<Vec<u32>>(),
        "the pages are the runs the keys spell, in order"
    );
    // The runs are contiguous: read in the member's own order, no page starts
    // again after another page began, and the runs are the pages 0..=24 in
    // order. This is the shape `DiscoveredPage` and the finding record as
    // measured, so a grouping that scattered one page through the member fails
    // here instead of being believed.
    let mut in_member_order: Vec<(u64, u32)> = discovered
        .pages
        .iter()
        .flat_map(|page| page.items.iter().map(|item| (item.line, page.page)))
        .collect();
    in_member_order.sort();
    let mut runs: Vec<u32> = Vec::new();
    for (_, page) in &in_member_order {
        if runs.last() != Some(page) {
            runs.push(*page);
        }
    }
    assert_eq!(
        runs,
        (0..=24).collect::<Vec<u32>>(),
        "every page is one contiguous run of the member, and the runs are 0..=24 in order"
    );
    assert_eq!(
        discovered.gaps.len(),
        1,
        "the retail table produces exactly one named gap"
    );
    assert_eq!(
        discovered.items_with_artwork(),
        294,
        "the items the container holds a picture for"
    );
    assert_eq!(
        gaps_of_report(&discovered.gaps),
        vec![
            ("ambiguous_entry_key".to_owned(), 0),
            ("artwork_outside_artwork_directory".to_owned(), 0),
            ("duplicate_entry_key".to_owned(), 0),
            ("entry_key_not_page_structured".to_owned(), 0),
            ("entry_not_a_scrapbook_item".to_owned(), 0),
            ("item_without_artwork".to_owned(), 167),
            ("entry_key_not_utf8".to_owned(), 0),
            ("artwork_undecodable_format".to_owned(), 0),
        ],
        "167 items name a picture the installation does not hold, and nothing else is refused"
    );
    assert_eq!(
        discovered.pages_without_artwork(),
        Vec::<u32>::new(),
        "every page holds at least one picture the container has"
    );
    for page in &discovered.pages {
        for item in &page.items {
            assert!(
                item.objective.is_some(),
                "{}: the Objective field is a whole number in the retail table",
                item.key
            );
            if let Some(capture) = &item.capture {
                assert!(
                    capture.starts_with("ASSETS/GRAPHICS/SCRAPBOOK/"),
                    "{}: the picture lives in the container's artwork directory: {capture}",
                    item.key
                );
            }
        }
    }
    assert_eq!(
        discovered.memento_named_images(),
        15,
        "the table's images shaped like the memento-selection script's own picture name"
    );
    let json = discovered.json();
    assert_parses_as_json(&json);
    assert!(json.contains("\"records\": 461"), "{json}");

    // The audit against the real progression: the production baseline's
    // mission rows and its own `scrapbook_item` rows, which are this stage's
    // discovery read by a different path and must agree on identity.
    let baseline = retail_baseline(&game_dir).expect("the production baseline reads");
    let catalog_rows: BTreeSet<String> = baseline
        .catalog
        .elements()
        .filter(|element| element.kind == ContentKind::ScrapbookItem)
        .map(|element| element.id.key().to_owned())
        .collect();
    let discovered_keys: BTreeSet<String> = discovered
        .pages
        .iter()
        .flat_map(|page| &page.items)
        .filter_map(|item| item.id_key.clone())
        .collect();
    assert_eq!(
        discovered_keys, catalog_rows,
        "F14-D.8's catalog rows and this discovery name the same 461 items"
    );

    let declared = ScrapbookCatalog::new(Vec::new()).expect("an empty declaration");
    let report = audit(&discovered, &declared, &baseline.catalog);
    assert_eq!(report.progression_missions, 24, "the campaign's missions");
    assert_eq!(report.declared, 0, "the runtime declares no entry yet");
    assert_eq!(
        report.undeclared_items, 461,
        "every original item is undeclared: the gap is reported, never hidden"
    );
    assert_eq!(report.unlock_known, 0);
    assert_eq!(report.replay_links, 0);
    assert_eq!(report.declared_mementos, 0);
    assert_eq!(report.pages, 25);
    assert_eq!(report.items_without_artwork, 167);
    assert!(
        !report.is_complete(),
        "the audit must report the declared catalog as incomplete, not pass"
    );
    let json = report.json();
    assert_parses_as_json(&json);
    for needle in [
        "\"records\": 461",
        "\"pages\": 25",
        "\"progression_missions\": 24",
        "\"undeclared_items\": 461",
        "\"original_unlock_fields\": 0",
        "\"original_replay_fields\": 0",
        "\"original_memento_records\": 0",
    ] {
        assert!(
            json.contains(needle),
            "the artifact is missing {needle:?}:\n{json}"
        );
    }
}

/// SHA-256 of the decoded `ASSETS/SCRAPBOOK.CSV` member of the owner's
/// installation, measured by every stage that has read it (F12-D, F12-H,
/// F12-I, F14-D.8).
const EXPECTED_MEMBER_SHA256: &str =
    "28b5144c54120f52c36717a3f1e094cb75845ecb1f854334a5686d5f6c6af5c1";
