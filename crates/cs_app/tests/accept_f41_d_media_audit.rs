//! F41-D: the original-media audit of every sound container (task #653).
//!
//! Needs `CS_GAME_DIR`; run with `--include-ignored`. The tests fail loudly
//! without it.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use cs_app::audio::audit::{
    ContainerAudit, MemberReadiness, SAMPLES_PER_SHAPE, Shape, audit_container,
    sound_container_spellings,
};
use cs_assets::install::{discover, fingerprint};
use cs_assets::vfs::ContentSession;

/// The audit decodes every member of both archives, which is slow in a debug
/// build, so the tests share one run of it.
fn audit_installation() -> &'static [ContainerAudit] {
    static AUDIT: OnceLock<Vec<ContainerAudit>> = OnceLock::new();
    AUDIT.get_or_init(run_audit)
}

fn run_audit() -> Vec<ContainerAudit> {
    let root = PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must point at the original installation"),
    );
    let found = discover(&root).expect("production discovery reads the installation");
    let context = cs_types::asset_id::ResolveContext::new(fingerprint(&found.manifest));
    let mut builder = cs_assets::vfs::SessionBuilder::new(context);
    builder
        .mount_installation(&root, &found.diagnosis)
        .expect("the installation mounts");
    let session: ContentSession = builder.open();
    let spellings = sound_container_spellings(&found.manifest.files);
    spellings
        .iter()
        .map(|spelling| {
            audit_container(&session, spelling)
                .unwrap_or_else(|refusal| panic!("{spelling} was refused: {refusal:?}"))
        })
        .collect()
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f41_d_every_member_of_every_container_has_a_reconciling_row() {
    let audits = audit_installation();
    let names: Vec<&str> = audits.iter().map(|a| a.container.as_str()).collect();
    assert_eq!(names, ["ZBD/soundsh.zbd", "ZBD/soundsl.zbd"]);
    let mut total_declared = 0;
    for audit in audits {
        // Measured by F14-D.7 straight from each archive's own index.
        let expected = if audit.container.ends_with("soundsl.zbd") {
            2520
        } else {
            2521
        };
        assert_eq!(audit.declared, expected, "{}", audit.container);
        assert!(audit.reconciles(), "{} does not reconcile", audit.container);
        // Position is the row's own identity: strictly increasing, no gaps lost.
        for pair in audit.rows.windows(2) {
            assert!(pair[0].index < pair[1].index);
        }
        total_declared += audit.declared;
        eprintln!(
            "{}: declared {} rows {} decoded {} refusals {:?} unreadable {:?}",
            audit.container,
            audit.declared,
            audit.rows.len(),
            audit.decoded(),
            audit.refusals(),
            audit.unreadable_extent
        );
        for (shape, count) in audit.shapes() {
            eprintln!("  shape {shape:?}: {count} members");
        }
        // Every member whose name keys has the F14-D.7 identity; the rest say why.
        let keyed = audit.rows.iter().filter(|row| row.id.is_ok()).count();
        eprintln!("  keyed {keyed} / {}", audit.rows.len());
        for row in audit.rows.iter().filter(|row| row.id.is_err()) {
            eprintln!("  unkeyed {}: {:?}", row.name, row.id);
        }
    }
    assert_eq!(total_declared, 5041);
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f41_d_refusals_are_counted_by_their_own_code_never_dropped() {
    for audit in audit_installation() {
        let refused: usize = audit.refusals().values().sum();
        let unreadable: usize = audit.unreadable_extent.values().sum();
        assert_eq!(
            audit.decoded() + refused + unreadable,
            audit.declared,
            "{}",
            audit.container
        );
        for row in &audit.rows {
            if let MemberReadiness::Refused { code } = &row.readiness {
                assert!(!code.is_empty(), "{} #{}", audit.container, row.index);
            }
        }
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f41_d_each_shape_has_a_second_distinct_playable_sample() {
    for audit in audit_installation() {
        let mut decoded_by_shape: BTreeMap<Shape, usize> = BTreeMap::new();
        for row in &audit.rows {
            if matches!(row.readiness, MemberReadiness::Decoded { .. })
                && let Some(shape) = row.shape()
            {
                *decoded_by_shape.entry(shape).or_default() += 1;
            }
        }
        for (shape, decoded) in &decoded_by_shape {
            let kept = audit.playable.get(shape).map_or(0, Vec::len);
            eprintln!(
                "{} {shape:?}: {decoded} decoded, {kept} playable samples kept",
                audit.container
            );
            assert!(
                kept >= 1,
                "{} {shape:?} has no playable sample",
                audit.container
            );
            if *decoded >= SAMPLES_PER_SHAPE {
                // A second sample exists whenever a second distinct, non-silent
                // member does; report rather than assume when it does not.
                if kept < SAMPLES_PER_SHAPE {
                    eprintln!("  only {kept}: the other members repeat or are silent");
                }
            }
            for sample in &audit.playable[shape] {
                assert!(sample.peak > 0 && sample.samples > 0);
                eprintln!(
                    "  #{} {} samples {} peak {}",
                    sample.index, sample.name, sample.samples, sample.peak
                );
            }
            let indices: Vec<usize> = audit.playable[shape].iter().map(|s| s.index).collect();
            assert!(
                indices.windows(2).all(|pair| pair[0] != pair[1]),
                "a shape's samples are distinct members"
            );
        }
    }
}
