//! Retail facts behind the original engine's reader-member lookup order
//! (task #341, `docs/findings/2026-10-05-f04-d-original-lookup-order.md`).
//!
//! The owner's static analysis of the original executable says the reader
//! mounts `[root, mission, world]` archives, reduces a requested name to its
//! basename and serves the first case-insensitive match in mount order. This
//! test reads every `zrdr.zbd` of `$CS_GAME_DIR` read-only through the
//! production reader-archive parser and pins what that order means for this
//! installation: the five mission-over-world shadowing cases, the absence of
//! root collisions and the two `player.zrd` members of the root archive.
//! It fails loudly without `CS_GAME_DIR`; CI skips it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cs_assets::install::sha256;
use cs_formats::io::ParseContext;
use cs_formats::zbd::{ZbdProbe, dispatch, read_reader_archive, read_version_one_index};
use cs_types::install::RelativePath;

/// The acceptance prefix of this task.
const _PREFIX: &str = "accept_f04_d_original_order_";

/// Members of one reader archive: lower-cased name -> (size, sha256 hex),
/// in declared index order.
type Members = Vec<(String, usize, String)>;

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var_os("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

fn read_members(path: &Path, label: &str) -> Members {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let spelling = RelativePath::new(label).expect("a valid spelling");
    let mut context = ParseContext::with_defaults(label);
    let decision = dispatch(ZbdProbe::new(label, &spelling, &bytes)).expect("reader family");
    let index = read_version_one_index(&mut context, decision, &bytes).expect("version-one index");
    let table = index.member_table();
    let archive = read_reader_archive(&mut context, &table, index.data()).expect("reader archive");
    archive
        .entries()
        .map(|e| {
            (
                String::from_utf8_lossy(e.name()).to_ascii_lowercase(),
                e.content().len(),
                sha256(e.content()).to_hex(),
            )
        })
        .collect()
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
    let root = read_members(&root_dir.join("zrdr.zbd"), "ZBD/zrdr.zbd");

    // Root: two player.zrd members; by-name lookup serves entry #22.
    let players: Vec<usize> = root
        .iter()
        .enumerate()
        .filter(|(_, m)| m.0 == "player.zrd")
        .map(|(i, _)| i)
        .collect();
    assert_eq!(players, vec![22, 100], "player.zrd positions in root");
    assert_eq!(root[22].1, 3414);
    assert!(root[22].2.starts_with("a8cc7547"), "{}", root[22].2);
    assert_eq!(root[100].1, 34711);
    assert_eq!(first_hit(&[&root], "player.zrd").unwrap().1, 3414);

    let mut worlds: BTreeMap<String, Members> = BTreeMap::new();
    let mut shadows: Vec<String> = Vec::new();
    let mut archives = 1;
    for world in ["C1", "C1B", "C1C", "C2", "C2B", "C3", "C4", "C5"] {
        let world_members = read_members(
            &root_dir.join(world).join("zrdr.zbd"),
            &format!("ZBD/{world}/zrdr.zbd"),
        );
        archives += 1;
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
            let mission_members = read_members(
                &root_dir.join(world).join(&mission).join("zrdr.zbd"),
                &format!("ZBD/{world}/{mission}/zrdr.zbd"),
            );
            archives += 1;
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
