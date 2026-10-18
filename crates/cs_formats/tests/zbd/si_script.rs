//! Task #1659 (`PARITY-M01-ATMO-E1`): the spline-interp key layout stored
//! inline in an animation container (`zbd::si_script`).
//!
//! The synthetic tests author their own bytes. The `#[ignore]`d ones read the
//! retail `C3/M01/mis_anim.zbd` (and sweep every animation container) from
//! `$CS_GAME_DIR`, read-only, and fail loudly without it.

use std::path::{Path, PathBuf};

use cs_formats::zbd::{
    SI_BLOCK_BYTES, SI_KEY_HEAD_BYTES, SI_TAG_ORIENTATION, SI_TAG_POSITION, SiKey, SiScript,
    SiScriptError, find_si_script, read_si_scripts,
};

const PATH: &[u8] = b"..\\data\\common\\zrdr\\zeps\\demo.zan\0";

fn block(start: &[f32], rate: [f32; 3], coefficients: [[f32; 4]; 3]) -> Vec<u8> {
    let mut out = Vec::new();
    for v in start.iter().chain(rate.iter()) {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for c in coefficients.iter().flatten() {
        out.extend_from_slice(&c.to_le_bytes());
    }
    assert_eq!(out.len(), SI_BLOCK_BYTES);
    out
}

fn key(tag: u32, t0: f32, t1: f32, blocks: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&tag.to_le_bytes());
    out.extend_from_slice(&t0.to_le_bytes());
    out.extend_from_slice(&t1.to_le_bytes());
    for b in blocks {
        out.extend_from_slice(b);
    }
    out
}

fn container(object: &[u8], keys: &[Vec<u8>]) -> Vec<u8> {
    let mut out = vec![0xAA; 16];
    out.extend_from_slice(PATH);
    out.extend_from_slice(object);
    out.push(0);
    for k in keys {
        out.extend_from_slice(k);
    }
    out.extend_from_slice(&[0xBB; 8]);
    out
}

fn linear(p: f32, v: f32) -> [f32; 4] {
    [p, v, 0.0, 0.0]
}

#[test]
fn accept_parity_m01_atmo_e1_synthetic_keys_decode_with_spans_and_evaluate() {
    let dt = 0.5f32;
    let pos = block(
        &[1.0, 2.0, 3.0, f32::from_bits(0x7c00_0001)],
        [2.0, 0.0, 0.0],
        [linear(1.0, 2.0), linear(2.0, 0.0), linear(3.0, 0.0)],
    );
    // Rotation vector (0, 0, a(s)) with a(s) = s: half-angle growth about z.
    let rot = block(
        &[1.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [[0.0; 4], [0.0; 4], [0.0, 1.0, 0.0, 0.0]],
    );
    let keys = [
        key(3, 0.0, dt, &[pos.clone(), rot.clone()]),
        key(0, dt, 1.0, &[]),
        key(1, 1.0, 1.5, std::slice::from_ref(&pos)),
        key(2, 1.5, 2.0, std::slice::from_ref(&rot)),
    ];
    let bytes = container(b"player", &keys);
    let scripts = read_si_scripts(&bytes);
    assert_eq!(scripts.len(), 1);
    let s = &scripts[0];
    assert_eq!(s.name(), "demo");
    assert_eq!(s.object(), b"player");
    assert_eq!(s.path_span().offset, 16);
    assert_eq!(s.keys().len(), 4);
    let sizes: Vec<u64> = s.keys().iter().map(|k| k.span.length).collect();
    let head = SI_KEY_HEAD_BYTES as u64;
    let blk = SI_BLOCK_BYTES as u64;
    assert_eq!(sizes, [head + 2 * blk, head, head + blk, head + blk]);
    // Keys tile: each starts where the previous ended; the script ends before the 0xBB tail.
    for pair in s.keys().windows(2) {
        assert_eq!(
            pair[0].span.offset + pair[0].span.length,
            pair[1].span.offset
        );
    }
    assert_eq!(s.span().offset + s.span().length, bytes.len() as u64 - 8);
    assert_eq!(s.duration(), 2.0);

    let k0 = &s.keys()[0];
    let p = k0.position.unwrap();
    assert_eq!(p.start, [1.0, 2.0, 3.0]);
    assert_eq!(p.padding_word, 0x7c00_0001);
    assert_eq!(p.at(dt), [2.0, 2.0, 3.0]);
    let q = k0.orientation.unwrap().at(dt);
    let half = dt;
    assert!((q[0] - half.cos()).abs() < 1e-6 && (q[3] - half.sin()).abs() < 1e-6);
    assert!(s.keys()[1].position.is_none() && s.keys()[1].orientation.is_none());
    assert!(s.keys()[2].orientation.is_none() && s.keys()[3].position.is_none());
    assert_eq!(s.keys()[2].tag & SI_TAG_POSITION, SI_TAG_POSITION);
    assert_eq!(s.keys()[3].tag & SI_TAG_ORIENTATION, SI_TAG_ORIENTATION);
}

#[test]
fn accept_parity_m01_atmo_e1_synthetic_chain_breaks_where_the_layout_does() {
    let pos = block(&[0.0; 4], [0.0; 3], [[0.0; 4]; 3]);
    // Second key's t0 is not the first key's t1: the script is the first key only.
    let bytes = container(
        b"o",
        &[
            key(1, 0.0, 0.5, std::slice::from_ref(&pos)),
            key(1, 0.6, 1.0, std::slice::from_ref(&pos)),
        ],
    );
    let scripts = read_si_scripts(&bytes);
    assert_eq!(scripts[0].keys().len(), 1);
    // A first key that does not start at 0 is not a script.
    assert!(read_si_scripts(&container(b"o", &[key(0, 0.25, 0.5, &[])])).is_empty());
    // A tag with an unmeasured bit is not a key.
    assert!(read_si_scripts(&container(b"o", &[key(4, 0.0, 0.5, &[])])).is_empty());
    // An empty object name (a member-table row) is not a script.
    assert!(read_si_scripts(&container(b"", &[key(0, 0.0, 0.5, &[])])).is_empty());
    // A truncated block ends the chain instead of reading past the bytes.
    let mut cut = container(b"o", &[key(1, 0.0, 0.5, std::slice::from_ref(&pos))]);
    cut.truncate(cut.len() - 8 - 4);
    assert!(read_si_scripts(&cut).is_empty());
}

#[test]
fn accept_parity_m01_atmo_e1_synthetic_lookup_names_missing_and_ambiguous_scripts() {
    let one = key(0, 0.0, 0.5, &[]);
    let mut bytes = container(b"a", std::slice::from_ref(&one));
    bytes.extend_from_slice(PATH);
    bytes.extend_from_slice(b"b\0");
    bytes.extend_from_slice(&one);
    assert_eq!(
        find_si_script(&bytes, "DEMO", Some("b")).unwrap().object(),
        b"b"
    );
    assert!(matches!(
        find_si_script(&bytes, "demo", None),
        Err(SiScriptError::Ambiguous { .. })
    ));
    assert!(matches!(
        find_si_script(&bytes, "other", None),
        Err(SiScriptError::NotFound { .. })
    ));
}

fn game_dir() -> PathBuf {
    let dir = PathBuf::from(
        std::env::var_os("CS_GAME_DIR")
            .expect("CS_GAME_DIR is not set: this test needs the original installation"),
    );
    assert!(
        dir.is_dir(),
        "CS_GAME_DIR is not a directory: {}",
        dir.display()
    );
    dir
}

fn m01() -> Vec<u8> {
    std::fs::read(game_dir().join("ZBD/C3/M01/mis_anim.zbd")).unwrap()
}

/// Largest disagreement between a key's evaluated end state and the next key's
/// stored start, for the channels both keys carry.
fn continuity_error(a: &SiKey, b: &SiKey) -> (f32, f32) {
    let dt = a.t1 - a.t0;
    let mut pos = 0.0f32;
    let mut rot = 0.0f32;
    if let (Some(x), Some(y)) = (&a.position, &b.position) {
        for (e, s) in x.at(dt).iter().zip(y.start) {
            pos = pos.max((e - s).abs() / s.abs().max(1.0));
        }
        for ((rate, e), s) in x.mean_rate.iter().zip(x.at(dt)).zip(x.start) {
            pos = pos.max(((e - s) / dt - rate).abs() / rate.abs().max(1.0));
        }
    }
    if let (Some(x), Some(y)) = (&a.orientation, &b.orientation) {
        // `q` and `-q` are one rotation; retail stores either sign for the next key.
        let end = x.at(dt);
        let direct = end
            .iter()
            .zip(y.start)
            .map(|(e, s)| (e - s).abs())
            .fold(0.0, f32::max);
        let flipped = end
            .iter()
            .zip(y.start)
            .map(|(e, s)| (e + s).abs())
            .fold(0.0, f32::max);
        rot = rot.max(direct.min(flipped));
    }
    (pos, rot)
}

fn script<'a>(bytes: &'a [u8], name: &str, object: &str) -> SiScript<'a> {
    find_si_script(bytes, name, Some(object)).unwrap()
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_atmo_e1_retail_m01_scripts_tile_and_the_launch_player_decodes() {
    let bytes = m01();
    let scripts = read_si_scripts(&bytes);
    let inventory: Vec<(String, String, usize)> = scripts
        .iter()
        .map(|s| {
            (
                s.name(),
                String::from_utf8_lossy(s.object()).into_owned(),
                s.keys().len(),
            )
        })
        .collect();
    let expected = [
        ("td_player1", "player", 8),
        ("td_player2", "player", 10),
        ("td_cam", "camera1", 18),
        ("td_chute", "chutemanparent", 18),
        ("td_chute", "pilot", 18),
        ("pzhookup_cam1a", "camera1", 12),
        ("pzhookup_cam1", "camera1", 43),
        ("pzhookup_cam2", "camera1", 48),
        ("pzhookup_cam1b", "camera1", 40),
        ("pzhookup_playerhold", "player", 12),
        ("pzhookup_player", "player", 23),
        ("pzhookup_impact", "hook_base", 23),
        ("pzhookup_impact", "top_seg", 23),
    ];
    let expected: Vec<(String, String, usize)> = expected
        .iter()
        .map(|(a, b, c)| ((*a).into(), (*b).into(), *c))
        .collect();
    assert_eq!(inventory, expected);
    // From `pzhookup_cam1a` on, consecutive scripts abut: the keys tile with no gap and
    // no closing record. `td_chute/pilot` is followed by 1025 bytes this reader does not
    // walk before `pzhookup_cam1a` (other event data, unmeasured here).
    let from = scripts
        .iter()
        .position(|s| s.name() == "pzhookup_cam1a")
        .unwrap();
    for pair in scripts[from..].windows(2) {
        assert_eq!(
            pair[0].span().offset + pair[0].span().length,
            pair[1].path_span().offset,
            "{} / {}",
            pair[0].name(),
            pair[1].name()
        );
    }
    let pilot = &scripts[from - 1];
    assert_eq!(
        scripts[from].path_span().offset - (pilot.span().offset + pilot.span().length),
        1025
    );

    let player = script(&bytes, "pzhookup_player", "player");
    assert_eq!(player.span().offset, 902179);
    assert_eq!(player.keys().len(), 23);
    assert!(
        player
            .keys()
            .iter()
            .all(|k| k.tag == 3 && k.span.length == 164)
    );
    assert_eq!(player.keys()[0].t0, 0.0);
    assert!((player.duration() - 23.0 / 3.0).abs() < 1e-5);
    let first = player.keys()[0].position.unwrap();
    for (got, want) in first.start.iter().zip([-0.08879f32, -1.7995, 9.96528]) {
        assert!((got - want).abs() < 1e-4, "{got} vs {want}");
    }
    // c0 of each axis is the stored start, and the stored start is the polynomial at 0.
    assert_eq!(first.at(0.0), first.start);
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_atmo_e1_retail_m01_keys_are_continuous_under_the_measured_evaluators() {
    let bytes = m01();
    let mut pairs = 0;
    let (mut worst_pos, mut worst_rot) = (0.0f32, 0.0f32);
    for s in read_si_scripts(&bytes) {
        for pair in s.keys().windows(2) {
            let (p, r) = continuity_error(&pair[0], &pair[1]);
            assert!(
                p < 5e-6 && r < 2e-3,
                "{} @ {}: {p} {r}",
                s.name(),
                pair[1].span.offset
            );
            worst_pos = worst_pos.max(p);
            worst_rot = worst_rot.max(r);
            pairs += usize::from(pair[0].position.is_some() && pair[1].position.is_some());
        }
    }
    println!("M01 position pairs {pairs}, worst position {worst_pos}, worst rotation {worst_rot}");
    assert!(pairs > 100);
}

fn animation_containers(root: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            animation_containers(&path, out);
        } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            let name = name.to_ascii_lowercase();
            if name == "mis_anim.zbd" || name == "cam_anim.zbd" {
                out.push(path);
            }
        }
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_atmo_e1_retail_every_animation_container_obeys_the_same_layout() {
    let mut files = Vec::new();
    animation_containers(&game_dir().join("ZBD"), &mut files);
    assert!(files.len() >= 61, "{} containers", files.len());
    let (mut scripts, mut keys, mut tags) = (0usize, 0usize, [0usize; 4]);
    let mut worst = (0.0f32, 0.0f32);
    for file in files {
        let bytes = std::fs::read(&file).unwrap();
        let found = read_si_scripts(&bytes);
        for pair in found.windows(2) {
            assert!(
                pair[0].span().offset + pair[0].span().length <= pair[1].path_span().offset,
                "{}: overlapping scripts",
                file.display()
            );
        }
        for s in &found {
            scripts += 1;
            for pair in s.keys().windows(2) {
                let (p, r) = continuity_error(&pair[0], &pair[1]);
                worst = (worst.0.max(p), worst.1.max(r));
                assert!(
                    p < 5e-5 && r < 0.1,
                    "{} {} @ {}: {p} {r}",
                    file.display(),
                    s.name(),
                    pair[1].span.offset
                );
            }
            for k in s.keys() {
                keys += 1;
                tags[k.tag as usize] += 1;
            }
        }
    }
    println!("{scripts} scripts, {keys} keys, per tag {tags:?}, worst (pos, rot) {worst:?}");
    assert!(scripts >= 13 && tags.iter().all(|n| *n > 0));
}
