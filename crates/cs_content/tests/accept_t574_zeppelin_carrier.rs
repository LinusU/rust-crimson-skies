//! Task #574: the mission-scoped `zeppelins.zrd` carrier, decoded.
//!
//! The unignored tests author every byte (a `.zrd` member, a reader archive,
//! a minimal installation tree) and run the production decode, the
//! production census and the production neutral-traffic support evaluation,
//! so the mechanics — a mission-scoped carrier, an omission, an undecodable
//! member and the measured negative link to `DeclaredNeutralTraffic` — are
//! falsifiable in CI. The `#[ignore]`d tests read the real installation and
//! state the measured corpus.
//!
//! Nothing here is `verified_original`: no original executable ran, and
//! reading the installation's files is not evidence of how the original
//! behaves. See `docs/findings/2026-10-08-t574-*.md`.

use std::fs;
use std::path::{Path, PathBuf};

use cs_content::pilots::{
    CarrierCensusError, CarrierFieldSupport, NeutralTrafficField, neutral_traffic_support,
    survey_retail_zeppelin_carrier,
};
use cs_formats::zbd::zeppelins::ZEPPELINS_MEMBER;

// --- authored `.zrd` bytes (independent of the production decoder) ----------

fn int(value: u32) -> Vec<u8> {
    [1u32.to_le_bytes(), value.to_le_bytes()].concat()
}

fn float(value: f32) -> Vec<u8> {
    [2u32.to_le_bytes(), value.to_bits().to_le_bytes()].concat()
}

fn text(value: &str) -> Vec<u8> {
    let mut out = 3u32.to_le_bytes().to_vec();
    out.extend((value.len() as u32).to_le_bytes());
    out.extend(value.as_bytes());
    out
}

/// A list of `children`, stored with the measured word: children plus one.
fn list(children: &[Vec<u8>]) -> Vec<u8> {
    let mut out = 4u32.to_le_bytes().to_vec();
    out.extend((children.len() as u32 + 1).to_le_bytes());
    for child in children {
        out.extend(child);
    }
    out
}

fn one_text(value: &str) -> Vec<u8> {
    list(&[text(value)])
}

fn one_int(value: u32) -> Vec<u8> {
    list(&[int(value)])
}

fn one_float(value: f32) -> Vec<u8> {
    list(&[float(value)])
}

fn text_list(values: &[&str]) -> Vec<u8> {
    list(&values.iter().map(|v| text(v)).collect::<Vec<_>>())
}

/// The 16 required key/value pairs, with authored values.
fn required_pairs(node: &str) -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("node", one_text(node)),
        ("position", list(&[float(1.0), float(2.0), float(3.0)])),
        ("yaw", one_float(90.0)),
        ("pitch", one_float(0.0)),
        ("max_speed", one_float(5.0)),
        ("max_accel", one_float(4.47)),
        ("accel_pitch", one_float(0.5)),
        ("accel_yaw", one_float(0.5)),
        ("max_rate_yaw", one_float(5.0)),
        ("max_rate_pitch", one_float(5.0)),
        ("min_pitch", one_float(-30.0)),
        ("max_pitch", one_float(30.0)),
        ("net", one_text("TestNet")),
        ("healthy", list(&[list(&[text("gasbag1"), text("panels")])])),
        ("num_healthy_required", one_int(2)),
        ("engines", text_list(&["engine1"])),
    ]
}

/// A record node stating the required keys and `extra`.
fn record(node: &str, extra: &[(&'static str, Vec<u8>)]) -> Vec<u8> {
    let mut children = Vec::new();
    for (key, value) in required_pairs(node)
        .into_iter()
        .chain(extra.iter().cloned())
    {
        children.push(text(key));
        children.push(value);
    }
    list(&children)
}

/// A `zeppelins.zrd` member holding `records`.
fn member(records: &[Vec<u8>]) -> Vec<u8> {
    list(&[list(records)])
}

// --- authored reader archives and a throwaway installation -------------------

/// A version-one reader archive holding `members` in order: the member data,
/// then one 148-byte index entry each (u32 start, u32 length, a 64-byte
/// NUL-padded name and 76 bytes), then the u32 version `1` and u32 count.
///
/// Written independently of the production reader: the reader's own accepted
/// shape is what the census proves. The same independent writer F33-D
/// authored.
fn reader_archive(members: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut entries = Vec::with_capacity(members.len());
    for (name, member) in members {
        let start = bytes.len() as u32;
        bytes.extend_from_slice(member);
        entries.push((start, member.len() as u32, *name));
    }
    for (start, length, name) in &entries {
        bytes.extend_from_slice(&start.to_le_bytes());
        bytes.extend_from_slice(&length.to_le_bytes());
        let name = name.as_bytes();
        assert!(name.len() < 64, "a fixture member name fits its field");
        let mut field = [0_u8; 64];
        field[..name.len()].copy_from_slice(name);
        bytes.extend_from_slice(&field);
        bytes.extend_from_slice(&[0_u8; 76]);
    }
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&(members.len() as u32).to_le_bytes());
    bytes
}

/// A throwaway installation tree.
struct TempInstallation {
    root: PathBuf,
}

impl TempInstallation {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the system clock is after the Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "crimson-t574-{label}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("the fixture root is created");
        Self { root }
    }

    fn write(&self, spelling: &str, bytes: &[u8]) {
        let path = self.root.join(spelling);
        fs::create_dir_all(path.parent().expect("a fixture spelling has a parent"))
            .expect("the fixture directories are created");
        fs::write(&path, bytes).expect("the fixture bytes are written");
    }

    fn root(&self) -> &Path {
        &self.root
    }
}

impl Drop for TempInstallation {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// The installation-scope archives always exist and carry no carrier member.
fn write_empty_scope_archives(install: &TempInstallation, groups: &[&str]) {
    let other = reader_archive(&[("other.zrd", b"data".to_vec())]);
    install.write("ZBD/zrdr.zbd", &other);
    for group in groups {
        install.write(&format!("ZBD/{group}/zrdr.zbd"), &other);
    }
}

// ------------------------------------------------ the support evaluation ---

/// The measured negative: a decoded record supplies nothing the declared
/// neutral-traffic schema asks for. `can_lower` is `false`, the missing and
/// nearby fields are named, and a stated `team` stays mission vocabulary —
/// nearby `faction`, never a faction id.
#[test]
fn accept_t574_support_reports_the_measured_negative() {
    let decoded = cs_formats::zbd::zeppelins::read_zeppelins_member(&member(&[record(
        "testzep",
        &[("team", one_text("ally"))],
    )]))
    .expect("the authored member decodes");
    let support = neutral_traffic_support(&decoded.records()[0]);
    assert_eq!(support.node(), "testzep");
    assert_eq!(support.team(), Some("ally"));
    assert!(
        !support.can_lower(),
        "no measured record can lower into a declared neutral-traffic row"
    );
    assert_eq!(support.supplied().count(), 0);
    assert_eq!(
        support.missing().collect::<Vec<_>>(),
        vec![
            NeutralTrafficField::Traffic,
            NeutralTrafficField::Pilot,
            NeutralTrafficField::Survivability,
        ]
    );
    assert_eq!(
        support.nearby().collect::<Vec<_>>(),
        vec![NeutralTrafficField::Airframe, NeutralTrafficField::Faction]
    );

    // The nearby entries name the key a naive wiring would grab and why it
    // is not the input.
    match support.support_for(NeutralTrafficField::Airframe) {
        CarrierFieldSupport::Nearby { key, note } => {
            assert_eq!(key.spelling(), "node");
            assert!(note.contains("world-node"), "{note}");
        }
        other => panic!("airframe must be nearby, not {other:?}"),
    }
    match support.support_for(NeutralTrafficField::Faction) {
        CarrierFieldSupport::Nearby { key, note } => {
            assert_eq!(key.spelling(), "team");
            assert!(note.contains("ally"), "{note}");
        }
        other => panic!("a stated team must be nearby, not {other:?}"),
    }

    // Without a `team`, even faction's nearby vocabulary is absent.
    let decoded =
        cs_formats::zbd::zeppelins::read_zeppelins_member(&member(&[record("testzep", &[])]))
            .expect("the authored member decodes");
    let support = neutral_traffic_support(&decoded.records()[0]);
    assert_eq!(support.team(), None);
    assert_eq!(
        support.support_for(NeutralTrafficField::Faction),
        CarrierFieldSupport::Absent
    );
    assert!(!support.can_lower());
}

// ------------------------------------------------------------- the census ---

/// The census decodes each mission's own archive: one carries the member
/// with a record, one carries it empty (present with zero records, the
/// measured multiplayer state), one omits it, and the installation-scope
/// archives carry none — so presence, emptiness and absence are three
/// distinguishable states.
#[test]
fn accept_t574_census_decodes_authored_mission_carriers() {
    let install = TempInstallation::new("census");
    install.write("ZBD/planes.zbd", b"authored fixture");
    write_empty_scope_archives(&install, &["C1", "C2"]);
    install.write(
        "ZBD/C1/M01/zrdr.zbd",
        &reader_archive(&[(ZEPPELINS_MEMBER, member(&[record("testzep", &[])]))]),
    );
    install.write(
        "ZBD/C1/MP1/zrdr.zbd",
        &reader_archive(&[(ZEPPELINS_MEMBER, member(&[]))]),
    );
    install.write(
        "ZBD/C2/M02/zrdr.zbd",
        &reader_archive(&[("other.zrd", b"data".to_vec())]),
    );

    let census =
        survey_retail_zeppelin_carrier(install.root()).expect("the authored installation surveys");
    assert_eq!(census.carrier_member(), ZEPPELINS_MEMBER);
    assert_eq!(census.missions().len(), 3, "one row per mission directory");
    assert!(!census.installation_scope_carrier());
    assert_eq!(census.record_count(), 1);
    assert!(census.team_spellings().is_empty());

    let rows: Vec<(String, String, bool, usize)> = census
        .missions()
        .iter()
        .map(|row| {
            (
                row.group().to_owned(),
                row.mission().to_owned(),
                row.carrier_present(),
                row.record_count(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            ("c1".to_owned(), "m01".to_owned(), true, 1),
            ("c1".to_owned(), "mp1".to_owned(), true, 0),
            ("c2".to_owned(), "m02".to_owned(), false, 0),
        ],
        "present-with-records, present-but-empty and absent stay distinct"
    );

    let omissions: Vec<(String, String)> = census
        .missions_without_carrier()
        .map(|row| (row.group().to_owned(), row.mission().to_owned()))
        .collect();
    assert_eq!(omissions, vec![("c2".to_owned(), "m02".to_owned())]);

    // The decoded record is reachable per mission, with its fields.
    let records: Vec<(&str, &str)> = census
        .records()
        .map(|(row, record)| (row.mission(), record.node()))
        .collect();
    assert_eq!(records, vec![("m01", "testzep")]);

    assert_eq!(census.install_sha256().len(), 64);
    assert_eq!(census.content_sha256().len(), 64);
    for row in census.missions() {
        assert_eq!(row.archive_sha256().len(), 64);
        assert_eq!(
            row.archive(),
            &format!("zbd/{}/{}/zrdr.zbd", row.group(), row.mission())
        );
    }
}

/// A synthetic installation-scope carrier is detected — and decoded, the
/// same measurement the mission rows get.
#[test]
fn accept_t574_census_reports_an_installation_scope_carrier() {
    let install = TempInstallation::new("scope");
    install.write("ZBD/planes.zbd", b"authored fixture");
    write_empty_scope_archives(&install, &["C2"]);
    install.write(
        "ZBD/C1/zrdr.zbd",
        &reader_archive(&[(ZEPPELINS_MEMBER, member(&[record("scopezep", &[])]))]),
    );
    install.write(
        "ZBD/C1/M01/zrdr.zbd",
        &reader_archive(&[("other.zrd", b"data".to_vec())]),
    );

    let census =
        survey_retail_zeppelin_carrier(install.root()).expect("the authored installation surveys");
    assert!(
        census.installation_scope_carrier(),
        "ZBD/C1/zrdr.zbd carries the member"
    );
    assert_eq!(census.missions_without_carrier().count(), 1);
}

/// A member that refuses to decode is a named failure naming its container —
/// never a silently skipped member.
#[test]
fn accept_t574_census_refuses_an_undecodable_member() {
    let install = TempInstallation::new("undecodable");
    install.write("ZBD/planes.zbd", b"authored fixture");
    write_empty_scope_archives(&install, &["C1"]);
    install.write(
        "ZBD/C1/M01/zrdr.zbd",
        &reader_archive(&[(ZEPPELINS_MEMBER, b"not a zrd member".to_vec())]),
    );

    match survey_retail_zeppelin_carrier(install.root()) {
        Err(CarrierCensusError::Decode { container, error }) => {
            assert_eq!(container, "zbd/c1/m01/zrdr.zbd");
            assert!(
                !error.to_string().is_empty(),
                "the decode failure names why"
            );
        }
        other => panic!("an undecodable member must refuse, got {other:?}"),
    }
}

/// An archive that is not a reader container at all cannot claim "the member
/// is absent" — it is a named refusal.
#[test]
fn accept_t574_census_refuses_a_non_reader_archive() {
    let install = TempInstallation::new("nonreader");
    install.write("ZBD/planes.zbd", b"authored fixture");
    write_empty_scope_archives(&install, &["C1"]);
    install.write("ZBD/C1/M01/zrdr.zbd", b"not a reader archive");

    match survey_retail_zeppelin_carrier(install.root()) {
        Err(
            CarrierCensusError::NotAReaderArchive { container, .. }
            | CarrierCensusError::Read { container, .. },
        ) => {
            assert_eq!(container, "zbd/c1/m01/zrdr.zbd");
        }
        other => panic!("a non-reader archive must refuse, got {other:?}"),
    }
}

/// The census refuses an installation it cannot discover instead of
/// reporting a shorter, quiet list.
#[test]
fn accept_t574_census_refuses_an_undiscoverable_installation() {
    let missing = std::env::temp_dir().join(format!(
        "crimson-t574-missing-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock is after 1970")
            .as_nanos()
    ));
    match survey_retail_zeppelin_carrier(&missing) {
        Err(CarrierCensusError::Discovery(_)) => {}
        other => panic!("a missing installation must refuse, got {other:?}"),
    }
}

// ----------------------------------------------------------- the retail read ---

fn retail_root() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR is not set: this measurement needs the original installation"),
    )
}

/// The measured corpus of the owner's installation: 53 mission directories,
/// 50 carrying the member (58 decoded records in all), three named omissions
/// and no installation-scope carrier. Counted and decoded through production
/// discovery, production archive reading and the production decoder — never
/// trusted from a previous run.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t574_retail_carrier_census_decodes_all_50_members() {
    let census =
        survey_retail_zeppelin_carrier(&retail_root()).expect("the owner's installation surveys");

    // The fingerprints that bind the numbers to this installation.
    assert_eq!(census.install_sha256().len(), 64);
    assert_eq!(census.content_sha256().len(), 64);

    // 53 mission directories, one row each.
    assert_eq!(
        census.missions().len(),
        53,
        "every mission directory reports a row"
    );
    assert_eq!(
        census.missions_with_carrier().count(),
        50,
        "50 mission archives carry the member"
    );

    // The three omissions, named.
    let omissions: Vec<(String, String)> = census
        .missions_without_carrier()
        .map(|row| (row.group().to_owned(), row.mission().to_owned()))
        .collect();
    assert_eq!(
        omissions,
        vec![
            ("c1".to_owned(), "m02".to_owned()),
            ("c2".to_owned(), "m01".to_owned()),
            ("c5".to_owned(), "mp2".to_owned()),
        ]
    );

    // Every carrying member decoded: 58 records in all, with the measured
    // per-member histogram — 12 empty (the c*/mp1 and c*/mp2 missions),
    // 23 with one record, 11 with two, three with three, one with four.
    assert_eq!(census.record_count(), 58);
    let mut histogram = std::collections::BTreeMap::new();
    for row in census.missions_with_carrier() {
        *histogram.entry(row.record_count()).or_insert(0usize) += 1;
    }
    assert_eq!(
        histogram,
        std::collections::BTreeMap::from([(0, 12), (1, 23), (2, 11), (3, 3), (4, 1)])
    );

    // No installation-scope archive carries the member: nothing a global
    // population could fall back on.
    assert!(
        !census.installation_scope_carrier(),
        "no ZBD/zrdr.zbd or ZBD/<group>/zrdr.zbd carries the member"
    );

    // The measured team vocabulary, verbatim.
    assert_eq!(
        census.team_spellings(),
        &std::collections::BTreeMap::from([("ally".to_owned(), 12u64), ("enemy".to_owned(), 4u64)])
    );

    // And the measured negative holds over the whole corpus: no decoded
    // record can lower into the declared neutral-traffic schema.
    assert_eq!(census.records().count(), 58);
    for (_, record) in census.records() {
        let support = neutral_traffic_support(record);
        assert!(
            !support.can_lower(),
            "record {} must not lower into declared neutral traffic",
            record.node()
        );
        assert_eq!(support.supplied().count(), 0);
    }
}

/// The measured record vocabulary over the corpus: every non-`player`
/// `targets` spelling resolves to a sibling record's `node` in the same
/// member, `team` stays inside its two spellings, `healthy` attachments stay
/// `panels`, and node names do not repeat within a member. Per-record shape
/// the decoder enforces is asserted here as measurement, not fixture.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t574_retail_decoded_records_hold_the_measured_vocabulary() {
    let census =
        survey_retail_zeppelin_carrier(&retail_root()).expect("the owner's installation surveys");
    assert_eq!(census.record_count(), 58);

    for (row, member) in census
        .missions_with_carrier()
        .map(|row| (row, row.member().expect("carrying rows have members")))
    {
        let nodes: std::collections::BTreeSet<&str> =
            member.records().iter().map(|r| r.node()).collect();
        assert_eq!(
            nodes.len(),
            member.len(),
            "{}/{} repeats a node name",
            row.group(),
            row.mission()
        );
        for record in member.records() {
            assert!(!record.node().is_empty());
            if let Some(targets) = record.targets() {
                for target in targets {
                    assert!(
                        target == "player" || nodes.contains(target.as_str()),
                        "{}/{}: target {target} names no sibling node",
                        row.group(),
                        row.mission()
                    );
                }
            }
            if let Some(team) = record.team() {
                assert!(
                    team == "ally" || team == "enemy",
                    "{}/{}: unmeasured team spelling {team:?}",
                    row.group(),
                    row.mission()
                );
            }
            for binding in record.healthy() {
                assert_eq!(
                    binding.attachment(),
                    "panels",
                    "{}/{}: unmeasured healthy attachment",
                    row.group(),
                    row.mission()
                );
            }
            assert!(!record.engines().is_empty());
        }
    }
}
