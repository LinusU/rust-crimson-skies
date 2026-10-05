//! Retail facts behind the original engine's reader-member lookup order
//! (task #341, `docs/findings/2026-10-05-f04-d-original-lookup-order.md`).
//!
//! The owner's static analysis of the original executable says the reader
//! mounts `[root, mission, world]` archives, reduces a requested name to its
//! basename and serves the first case-insensitive match in mount order. This
//! test reads every `zrdr.zbd` of `$CS_GAME_DIR` read-only through the
//! production reader chain and pins what that order means for this
//! installation: the five mission-over-world shadowing cases, the absence of
//! root collisions, the two `player.zrd` members of the root archive, and the
//! geometry of the 148-byte index entries the original's directory reader
//! walks. It fails loudly without `CS_GAME_DIR`; CI skips it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use cs_assets::install::sha256;
use cs_formats::io::ParseContext;
use cs_formats::zbd::{
    EntryAnomaly, INDEX_NAME_BYTES, INDEX_UNEXPLAINED_BYTES, VersionOneIndex, ZbdProbe, dispatch,
    read_reader_archive, read_version_one_index,
};
use cs_types::install::RelativePath;

/// The acceptance prefix of this task.
const _PREFIX: &str = "accept_f04_d_original_order_";

/// The world groups of this installation (`ZBD/<group>`), the retail sets the
/// original's world table names.
const WORLDS: [&str; 8] = ["C1", "C1B", "C1C", "C2", "C2B", "C3", "C4", "C5"];

/// The `u32` every retail reader index entry carries between its name field and
/// the copy of that name (section F of the finding). **Measured on this
/// installation; what it encodes is unknown**, so nothing in production reads
/// it and this test only pins the bytes.
const RETAIL_ENTRY_WORD: u32 = 2;

/// Members of one reader archive: lower-cased name -> (size, sha256 hex),
/// in declared index order.
type Members = Vec<(String, usize, String)>;

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var_os("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

/// Checks the geometry of every 148-byte index entry of `index` and returns
/// how many there were.
///
/// The production reader exposes the 64-byte name field
/// ([`INDEX_NAME_BYTES`]) and the 76 bytes after it
/// ([`INDEX_UNEXPLAINED_BYTES`], deliberately uninterpreted). Section F of the
/// finding records what those 76 bytes hold on retail, read here through that
/// same production accessors so the claim cannot drift from the parser:
/// `u32 word`, then a byte-identical zero-padded copy of the name, then a
/// never-zero `u64`. A reader that sliced the region wrong, or an entry that
/// broke one of the fields the pinned source asserts, fails here.
fn check_entry_geometry(label: &str, index: &VersionOneIndex<'_>) -> usize {
    let stamp_start = 4 + INDEX_NAME_BYTES;
    assert_eq!(
        stamp_start + 8,
        INDEX_UNEXPLAINED_BYTES,
        "{label}: the trailing timestamp must close the unexplained region"
    );
    for entry in index.entries() {
        let anomalies: Vec<&str> = entry.anomalies().map(EntryAnomaly::code).collect();
        assert!(
            anomalies.is_empty(),
            "{label}: entry {} breaks {anomalies:?}",
            entry.index()
        );
        assert!(
            entry.name().len() < INDEX_NAME_BYTES,
            "{label}: entry {} has an unterminated name",
            entry.index()
        );
        let tail = entry.unexplained().bytes();
        assert_eq!(tail.len(), INDEX_UNEXPLAINED_BYTES, "{label}");
        let word = u32::from_le_bytes(tail[..4].try_into().expect("a 4-byte word"));
        assert_eq!(
            word,
            RETAIL_ENTRY_WORD,
            "{label}: entry {} word",
            entry.index()
        );
        let mut expected = [0u8; INDEX_NAME_BYTES];
        expected[..entry.name().len()].copy_from_slice(entry.name());
        assert!(
            tail[4..stamp_start] == expected,
            "{label}: entry {} does not copy its own name field",
            entry.index()
        );
        let stamp = u64::from_le_bytes(tail[stamp_start..].try_into().expect("an 8-byte stamp"));
        assert_ne!(
            stamp,
            0,
            "{label}: entry {} has no trailing timestamp",
            entry.index()
        );
    }
    index.entries().len()
}

/// Reads one reader archive: the members in index order, and the entry count
/// the geometry check above saw.
fn read_members(path: &Path, label: &str) -> (Members, usize) {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let spelling = RelativePath::new(label).expect("a valid spelling");
    let mut context = ParseContext::with_defaults(label);
    let decision = dispatch(ZbdProbe::new(label, &spelling, &bytes)).expect("reader family");
    let index = read_version_one_index(&mut context, decision, &bytes).expect("version-one index");
    let entries = check_entry_geometry(label, &index);
    let table = index.member_table();
    let archive = read_reader_archive(&mut context, &table, index.data()).expect("reader archive");
    let members = archive
        .entries()
        .map(|e| {
            (
                String::from_utf8_lossy(e.name()).to_ascii_lowercase(),
                e.content().len(),
                sha256(e.content()).to_hex(),
            )
        })
        .collect();
    (members, entries)
}

/// Names that appear more than once inside one archive, sorted. The original
/// serves the *first* index entry, so a duplicate is invisible to a lookup
/// unless it is counted.
fn duplicate_names(members: &Members) -> Vec<String> {
    let mut seen = BTreeMap::<&str, usize>::new();
    let mut duplicates = BTreeSet::new();
    for (name, ..) in members {
        let count = seen.entry(name.as_str()).or_default();
        *count += 1;
        if *count == 2 {
            duplicates.insert(name.clone());
        }
    }
    duplicates.into_iter().collect()
}

/// The original's open: first archive in mount order holding the name wins,
/// first entry inside it.
fn first_hit<'a>(mounted: &[&'a Members], name: &str) -> Option<&'a (String, usize, String)> {
    mounted
        .iter()
        .find_map(|archive| archive.iter().find(|m| m.0 == name))
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f04_d_original_order_retail_mission_shadows_world_in_five_cases() {
    let root_dir = game_dir().join("ZBD");
    let (root, mut entries) = read_members(&root_dir.join("zrdr.zbd"), "ZBD/zrdr.zbd");

    // Root: two player.zrd members; by-name lookup serves entry #22.
    let players: Vec<usize> = root
        .iter()
        .enumerate()
        .filter(|(_, m)| m.0 == "player.zrd")
        .map(|(i, _)| i)
        .collect();
    assert_eq!(players, vec![22, 100], "player.zrd positions in root");
    assert_eq!(
        duplicate_names(&root),
        vec!["player.zrd"],
        "root duplicates"
    );
    assert_eq!(root[22].1, 3414);
    assert!(root[22].2.starts_with("a8cc7547"), "{}", root[22].2);
    assert_eq!(root[100].1, 34711);
    assert_eq!(first_hit(&[&root], "player.zrd").unwrap().1, 3414);

    let mut worlds: BTreeMap<String, Members> = BTreeMap::new();
    let mut shadows: Vec<String> = Vec::new();
    let mut archives = 1;
    for world in WORLDS {
        let (world_members, world_entries) = read_members(
            &root_dir.join(world).join("zrdr.zbd"),
            &format!("ZBD/{world}/zrdr.zbd"),
        );
        archives += 1;
        entries += world_entries;
        assert!(
            duplicate_names(&world_members).is_empty(),
            "{world} holds a duplicate member name"
        );
        for m in &world_members {
            assert!(
                !root.iter().any(|r| r.0 == m.0),
                "{world} member {} collides with the root archive",
                m.0
            );
        }
        worlds.insert(world.to_owned(), world_members);
    }
    for world in worlds.keys() {
        let mut missions: Vec<_> = std::fs::read_dir(root_dir.join(world))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.path().join("zrdr.zbd").is_file())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        missions.sort();
        for mission in missions {
            let (mission_members, mission_entries) = read_members(
                &root_dir.join(world).join(&mission).join("zrdr.zbd"),
                &format!("ZBD/{world}/{mission}/zrdr.zbd"),
            );
            archives += 1;
            entries += mission_entries;
            assert!(
                duplicate_names(&mission_members).is_empty(),
                "{world}/{mission} holds a duplicate member name"
            );
            let world_members = &worlds[world];
            for m in &mission_members {
                assert!(!root.iter().any(|r| r.0 == m.0), "{mission} vs root");
                if let Some(w) = world_members.iter().find(|w| w.0 == m.0) {
                    // Mount order [root, mission, world]: the mission copy is served.
                    let served = first_hit(&[&root, &mission_members, world_members], &m.0);
                    assert_eq!(served.unwrap().2, m.2);
                    assert_ne!(m.2, w.2, "shadowed copies differ in bytes");
                    shadows.push(format!(
                        "{}/{} {}",
                        world.to_ascii_uppercase(),
                        mission.to_ascii_uppercase(),
                        m.0
                    ));
                }
            }
        }
    }
    shadows.sort();
    assert_eq!(archives, 62, "zrdr.zbd archives read");
    assert_eq!(entries, 1293, "index entries read");
    assert_eq!(
        shadows,
        vec![
            "C1C/IA1 targets.zrd",
            "C1C/MP1 targets.zrd",
            "C1C/MP3 targets.zrd",
            "C2/M01 security_destroy.zrd",
            "C3/M02 fueltruck.zrd",
        ]
    );
}
