//! Acceptance scenario F33-D: the neutral-traffic population and the original
//! installation's per-mission carried traffic.
//!
//! Spec: `specs/F33-wingmates-factions-neutral-traffic-and-pilot-identity.md`,
//! stage `### F33-D`. Task test prefix: `accept_f33_d_`. Shared contract:
//! `docs/contracts/CLI-EVIDENCE.md`.
//!
//! Minimum scenario (AC04): **neutral traffic omitted by the authored mission
//! is not spawned by a global population system.**
//!
//! These tests drive production code only:
//!
//! * [`cs_app::roster::build_neutral_population`] builds the session's neutral
//!   population from the lowered **authored** list and nothing else, and
//!   [`cs_app::roster::NeutralPopulation::register_into`] registers each
//!   authored actor under its authored role. A request for an unauthored
//!   traffic index is answered with `None`, never with a default actor, so the
//!   "global population" AC04 forbids is unreachable by construction.
//! * [`cs_app::roster::survey_retail_neutral_traffic`] reads the owner's
//!   installation through the production discovery and the production
//!   reader-archive discovery, one row per mission directory, and reports
//!   whether the mission's own archive carries the observed placed-traffic
//!   member `zeppelins.zrd` and whether any installation-scope archive does.
//!
//! The unignored tests author every byte (a reader archive, a minimal
//! installation tree) and run the production census, so its mechanics — a
//! mission-scoped carrier, an omission, and the installation-scope carrier the
//! rule must not fall back on — are falsifiable in CI. The `#[ignore]`d test
//! reads the real installation and states the measured corpus.
//!
//! Nothing here is `verified_original`: `zeppelins.zrd`'s role and encoding are
//! inferred from its name and are **not** decoded, no original executable ran,
//! and reading the installation's files is not evidence of how the game
//! behaves. See `docs/findings/2026-10-03-f33-d-*.md`.

use std::fs;
use std::path::{Path, PathBuf};

use cs_app::roster::{
    OBSERVED_PLACED_TRAFFIC_MEMBER, PopulationError, TrafficCensusError, build_neutral_population,
    lower_roster, survey_retail_neutral_traffic,
};
use cs_content::pilots::{DeclaredPilot, DeclaredRoster, declared_synthetic_roster};
use cs_sim::allies::{AlliesRoster, AllyRole, SurvivabilityPolicy};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

const SESSION: u64 = 97;
const CARRIER: &str = "zeppelins.zrd";

fn id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("test id is valid")
}

fn claim(key: &str) -> ClaimId {
    ClaimId::new(key).expect("test claim id is valid")
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, Provenance::designed(claim("f33d.test"))))
}

/// A declared mission roster that authors one pilot and **no** neutral
/// traffic: the AC04 input.
fn roster_without_neutral_traffic() -> DeclaredRoster {
    let pilot = DeclaredPilot::try_new(
        id(ContentKind::Pilot, "synthetic.nathan"),
        known(id(ContentKind::Voice, "synthetic.nathan")),
        Origin::SyntheticFixture,
        Provenance::designed(claim("f33d.test")),
    )
    .expect("the pilot is valid");
    DeclaredRoster::try_new(
        id(ContentKind::Mission, "m01"),
        Origin::SyntheticFixture,
        id(ContentKind::Faction, "synthetic.nathan"),
        vec![pilot],
        Vec::new(),
        Vec::new(),
        Provenance::designed(claim("f33d.test")),
    )
    .expect("the roster is valid")
}

// ---------------------------------------------------------------------- AC04 ---

/// AC04: a mission that authored no neutral traffic spawns none, and the
/// session's population answers every traffic index with `None`; the authored
/// fixture's one neutral binds to exactly its serial and role, and an
/// unauthored index stays absent.
#[test]
fn accept_f33_d_population_spawns_only_authored_neutral_traffic() {
    // The omitted mission: empty population, no default actor anywhere.
    let omitted = lower_roster(&roster_without_neutral_traffic()).expect("the roster lowers");
    let empty =
        build_neutral_population(SESSION, &omitted, &[]).expect("an omitted list needs no serial");
    assert!(
        empty.is_empty(),
        "the omitted mission has no neutral traffic"
    );
    for index in 1..=4 {
        assert!(
            empty.spawn(index).is_none(),
            "traffic #{index} was never authored: a global population must not fill it"
        );
        assert!(empty.actor(index).is_none());
    }
    let mut roster = AlliesRoster::new(SESSION);
    empty
        .register_into(&mut roster)
        .expect("the empty population registers cleanly");
    assert_eq!(roster.actors().count(), 0, "nothing was spawned");

    // The authored mission: exactly one neutral, at its own index.
    let authored = lower_roster(&declared_synthetic_roster()).expect("the fixture lowers");
    let populated =
        build_neutral_population(SESSION, &authored, &[4]).expect("one neutral takes one serial");
    assert_eq!(populated.len(), 1);
    let spawn = populated.spawn(1).expect("traffic #1 is authored");
    assert_eq!(spawn.actor.serial, 4);
    assert_eq!(
        spawn.faction.as_content().as_str(),
        "faction/synthetic.traders"
    );
    assert_eq!(spawn.survivability, SurvivabilityPolicy::ProtectedNeutral);
    assert!(populated.spawn(2).is_none());

    let mut roster = AlliesRoster::new(SESSION);
    populated
        .register_into(&mut roster)
        .expect("the authored neutral registers");
    assert_eq!(roster.role_of(&spawn.actor), Some(AllyRole::Neutral(1)));
    assert_eq!(
        roster.voice_of(&spawn.actor),
        None,
        "a neutral whose mission authored no voice speaks through none"
    );
}

/// The population refuses a serial-count mismatch and a zero serial rather
/// than silently dropping or inventing an actor.
#[test]
fn accept_f33_d_population_refuses_a_serial_count_mismatch() {
    let authored = lower_roster(&declared_synthetic_roster()).expect("the fixture lowers");
    assert_eq!(
        build_neutral_population(SESSION, &authored, &[]),
        Err(PopulationError::SerialCount {
            authored: 1,
            provided: 0,
        })
    );
    assert_eq!(
        build_neutral_population(SESSION, &authored, &[0]),
        Err(PopulationError::ZeroSerial { index: 0 })
    );
}

// --------------------------------------------------- the census production path ---

/// A version-one reader archive holding `members` in order: the member data,
/// then one 148-byte index entry each (u32 start, u32 length, a 64-byte
/// NUL-padded name and 76 bytes), then the u32 version `1` and u32 count.
///
/// Written independently of the production reader: the reader's own accepted
/// shape is what the census proves. The same independent writer task #463
/// authored for the F42 reader archive.
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
            "crimson-f33d-{label}-{}-{nanos}",
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

/// The installation-scope archives always exist and carry no observed member.
fn write_empty_scope_archives(install: &TempInstallation, groups: &[&str]) {
    let other = reader_archive(&[("other.zrd", b"data".to_vec())]);
    install.write("ZBD/zrdr.zbd", &other);
    for group in groups {
        install.write(&format!("ZBD/{group}/zrdr.zbd"), &other);
    }
}

/// The census reads each mission's own archive: one carries the observed
/// member, one omits it, and the installation-scope archives carry neither —
/// so the omission is visible and the rule has no global carrier to fall back
/// on.
#[test]
fn accept_f33_d_census_reads_mission_scoped_carriers() {
    let install = TempInstallation::new("census");
    install.write("ZBD/planes.zbd", b"authored fixture");
    write_empty_scope_archives(&install, &["C1", "C2"]);
    install.write(
        "ZBD/C1/M01/zrdr.zbd",
        &reader_archive(&[(CARRIER, b"mission traffic".to_vec())]),
    );
    install.write(
        "ZBD/C2/M02/zrdr.zbd",
        &reader_archive(&[("other.zrd", b"data".to_vec())]),
    );

    let census =
        survey_retail_neutral_traffic(install.root()).expect("the authored installation surveys");
    assert_eq!(census.carrier_member(), OBSERVED_PLACED_TRAFFIC_MEMBER);
    assert_eq!(census.len(), 2, "one row per mission directory");
    assert!(!census.is_empty());

    let rows: Vec<(String, String, bool, usize)> = census
        .missions()
        .iter()
        .map(|row| {
            (
                row.group().to_owned(),
                row.mission().to_owned(),
                row.carrier_present(),
                row.members(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            ("c1".to_owned(), "m01".to_owned(), true, 1),
            ("c2".to_owned(), "m02".to_owned(), false, 1),
        ],
        "the carrier is attributed to the mission that authored it"
    );

    let omissions: Vec<(String, String)> = census
        .missions_without_carrier()
        .map(|row| (row.group().to_owned(), row.mission().to_owned()))
        .collect();
    assert_eq!(omissions, vec![("c2".to_owned(), "m02".to_owned())]);
    assert!(
        !census.installation_scope_carrier(),
        "no installation-scope archive carries the observed member"
    );

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

/// A synthetic installation whose installation-scope archive carries the
/// observed member is detected: that is the global carrier the runtime
/// population must never be driven by. The mission rows still report their own
/// (absent) authorship, which is the distinction the census exists to make.
#[test]
fn accept_f33_d_census_detects_an_installation_scope_carrier() {
    let install = TempInstallation::new("scope");
    install.write("ZBD/planes.zbd", b"authored fixture");
    write_empty_scope_archives(&install, &["C2"]);
    install.write(
        "ZBD/C1/zrdr.zbd",
        &reader_archive(&[(CARRIER, b"global traffic".to_vec())]),
    );
    install.write(
        "ZBD/C1/M01/zrdr.zbd",
        &reader_archive(&[("other.zrd", b"data".to_vec())]),
    );

    let census =
        survey_retail_neutral_traffic(install.root()).expect("the authored installation surveys");
    assert!(
        census.installation_scope_carrier(),
        "ZBD/C1/zrdr.zbd carries the observed member"
    );
    assert_eq!(
        census.missions_without_carrier().count(),
        1,
        "the mission itself authored no such member"
    );
    assert!(!census.missions()[0].carrier_present());
}

/// The census refuses an installation it cannot discover instead of reporting
/// a shorter, quiet list. A missing root is a discovery failure, not "no
/// mission authored traffic".
#[test]
fn accept_f33_d_census_refuses_an_undiscoverable_installation() {
    let missing = std::env::temp_dir().join(format!(
        "crimson-f33d-missing-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock is after 1970")
            .as_nanos()
    ));
    match survey_retail_neutral_traffic(&missing) {
        Err(TrafficCensusError::Discovery(_)) => {}
        other => panic!("a missing installation must refuse, got {other:?}"),
    }
}

// ------------------------------------------------------------- the retail read ---

fn retail_root() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR is set for a retail test"))
}

/// The measured corpus of the owner's installation: 53 mission directories,
/// three of which omit the observed placed-traffic carrier, and no
/// installation-scope archive carries it. This is the observation F33-D
/// records; it asserts nothing about behavior the game did not run for.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f33_d_retail_installation_authors_mission_scoped_traffic() {
    let census = survey_retail_neutral_traffic(&retail_root()).expect("the retail corpus surveys");

    assert_eq!(census.carrier_member(), OBSERVED_PLACED_TRAFFIC_MEMBER);
    assert_eq!(
        census.len(),
        53,
        "the measured number of ZBD/<group>/<mission> directories"
    );

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
        ],
        "the three missions whose own archive omits the observed carrier"
    );
    assert_eq!(census.len() - omissions.len(), 50);

    assert!(
        !census.installation_scope_carrier(),
        "no installation-scope archive (ZBD/zrdr.zbd or ZBD/<group>/zrdr.zbd) \
         carries the observed member"
    );

    assert_eq!(census.install_sha256().len(), 64);
    assert_eq!(census.content_sha256().len(), 64);
    for row in census.missions() {
        assert_eq!(row.archive_sha256().len(), 64, "{}", row.archive());
        assert!(
            row.members() >= 1,
            "{} has no reader-archive members at all",
            row.archive()
        );
    }
}
