//! Acceptance scenarios F48-A: versioned profile/save schema, write phases
//! and recovery. Task test prefix: `accept_f48_a_`.
//!
//! Spec: `specs/F48-profiles-saves-settings-migration-and-recovery.md`,
//! stage `### F48-A`. Every value is newly authored synthetic data.

use cs_content::save::codec::{DecodeError, MAX_SAVE_BYTES, checksum, decode, encode};
use cs_content::save::store::{
    CommitError, MemoryStorage, RecoverError, RecoveryWarning, SaveFile, SavePhase, StorageError,
    commit, recover,
};
use cs_types::content::{ContentId, ContentKind};
use cs_types::profile::{
    ExtraField, FingerprintEntry, ProfileDocument, ProfileId, ProfileKind, ProfileRegistry,
    RecordEntry, Revision, SchemaVersion, SettingApply, SettingEntry,
};

fn pid(n: u64) -> ProfileId {
    ProfileId::new(n).expect("nonzero")
}

fn doc(revision: u64) -> ProfileDocument {
    let mut d = ProfileDocument::synthetic(pid(1), Revision(revision));
    d.campaign.run_id = Some("run-a".into());
    d.campaign.money_minor = 1_000 + revision;
    d.campaign
        .applied_outcomes
        .push(format!("outcome-{revision}"));
    d.blueprints
        .push(ContentId::from_source(ContentKind::Airframe, "synthetic-one").expect("id"));
    d.records.push(RecordEntry {
        key: "kills".into(),
        value: revision,
    });
    d.settings.push(SettingEntry {
        key: "display.mode".into(),
        apply: SettingApply::RestartRequired,
        value: "window=ed".into(),
    });
    d.fingerprints.push(FingerprintEntry {
        name: "catalog".into(),
        hash: 0xdead_beef_0123_4567,
    });
    d
}

/// Re-seals hand-edited text with a correct checksum line.
fn seal(body: &str) -> Vec<u8> {
    format!("{body}checksum={:016x}\n", checksum(body.as_bytes())).into_bytes()
}

#[test]
fn accept_f48_a_document_round_trips_exactly() {
    let d = doc(3);
    assert_eq!(decode(&encode(&d).expect("encode")).expect("decode"), d);
}

/// AC01: crash in every phase of the second commit; reopening always yields a
/// valid state, never loses both current and backup, and is either the old or
/// the new revision whole.
#[test]
fn accept_f48_a_interrupt_every_write_phase_keeps_a_valid_revision() {
    for phase in SavePhase::ALL {
        let mut storage = MemoryStorage::new();
        commit(&mut storage, &doc(1)).expect("first");
        commit(&mut storage, &doc(2)).expect("second");
        storage.crash_at(phase);
        let err = commit(&mut storage, &doc(3)).expect_err("interrupted");
        assert_eq!(err, CommitError::Storage(StorageError::Interrupted(phase)));

        let recovered = recover(&storage).expect("recover").expect("state");
        let revision = recovered.document.revision;
        assert!(
            revision == Revision(2) || revision == Revision(3),
            "{phase:?}: got {revision:?}"
        );
        assert_eq!(recovered.document, doc(revision.0), "{phase:?}: whole doc");

        // Not both current and backup lost: at least one other valid copy
        // besides the recovered one remains unless the commit completed.
        let valid_files = SaveFile::ALL
            .into_iter()
            .filter(|f| storage.file(*f).is_some_and(|bytes| decode(bytes).is_ok()))
            .count();
        assert!(valid_files >= 1, "{phase:?}");

        // A later commit after the crash succeeds and wins.
        commit(&mut storage, &doc(4)).expect("recommit");
        let after = recover(&storage).expect("recover").expect("state");
        assert_eq!(after.document.revision, Revision(4), "{phase:?}");
        assert_eq!(after.source, SaveFile::Current);
        assert!(
            storage
                .file(SaveFile::Backup)
                .is_some_and(|b| decode(b).is_ok()),
            "{phase:?}: backup valid after recommit"
        );
    }
}

/// A crash in the very first commit leaves either nothing or a whole
/// revision, never a half-document accepted as state.
#[test]
fn accept_f48_a_interrupt_first_commit_never_accepts_a_torn_file() {
    for phase in SavePhase::ALL {
        let mut storage = MemoryStorage::new();
        storage.crash_at(phase);
        // The first commit has no current file to rotate, so that phase is
        // never reached and the commit completes.
        let outcome = commit(&mut storage, &doc(1));
        assert_eq!(
            outcome.is_ok(),
            phase == SavePhase::RotateBackup,
            "{phase:?}"
        );
        match recover(&storage) {
            Ok(Some(r)) => assert_eq!(r.document, doc(1), "{phase:?}"),
            Ok(None) => panic!("{phase:?}: a torn temp must be reported, not ignored"),
            Err(RecoverError::NoValidSave { diagnostics }) => {
                assert!(
                    matches!(phase, SavePhase::WriteTemp | SavePhase::SyncTemp),
                    "{phase:?}"
                );
                assert_eq!(diagnostics.len(), 1);
            }
            Err(other) => panic!("{phase:?}: {other}"),
        }
    }
}

/// AC02 (schema level): a corrupted newest file recovers the backup with a
/// visible warning, and the corrupt file is not deleted.
#[test]
fn accept_f48_a_corrupt_current_recovers_backup_with_warning() {
    let mut storage = MemoryStorage::new();
    commit(&mut storage, &doc(1)).expect("first");
    commit(&mut storage, &doc(2)).expect("second");
    let mut bytes = storage.file(SaveFile::Current).expect("current").to_vec();
    bytes[40] ^= 0x55;
    storage.set_file(SaveFile::Current, Some(bytes.clone()));

    let recovered = recover(&storage).expect("recover").expect("state");
    assert_eq!(recovered.document, doc(1));
    assert_eq!(recovered.source, SaveFile::Backup);
    assert!(recovered.warnings.iter().any(|w| matches!(
        w,
        RecoveryWarning::Corrupt {
            file: SaveFile::Current,
            ..
        }
    )));
    assert!(recovered.warnings.contains(&RecoveryWarning::UsedFallback {
        source: SaveFile::Backup
    }));
    assert_eq!(storage.file(SaveFile::Current), Some(bytes.as_slice()));

    // Committing over a corrupt current must keep the good backup intact
    // until the new file is installed.
    commit(&mut storage, &doc(3)).expect("commit over corrupt");
    assert_eq!(
        recover(&storage).expect("r").expect("s").document.revision,
        Revision(3)
    );
    assert_eq!(
        decode(storage.file(SaveFile::Backup).expect("bak"))
            .expect("valid")
            .revision,
        Revision(1)
    );
}

#[test]
fn accept_f48_a_recovery_never_merges_two_files() {
    let mut storage = MemoryStorage::new();
    storage.set_file(SaveFile::Current, Some(encode(&doc(5)).expect("e")));
    storage.set_file(SaveFile::Backup, Some(encode(&doc(4)).expect("e")));
    let r = recover(&storage).expect("r").expect("s");
    assert_eq!(r.document, doc(5));
    assert!(r.warnings.is_empty());
}

#[test]
fn accept_f48_a_stale_commit_is_a_conflict_not_an_overwrite() {
    let mut storage = MemoryStorage::new();
    commit(&mut storage, &doc(2)).expect("c");
    let before = storage.clone();
    let err = commit(&mut storage, &doc(2)).expect_err("same revision");
    assert!(matches!(err, CommitError::RevisionConflict { .. }));
    let err = commit(&mut storage, &doc(1)).expect_err("older revision");
    assert!(matches!(err, CommitError::RevisionConflict { .. }));
    let mut other = doc(9);
    other.profile_id = pid(2);
    assert!(matches!(
        commit(&mut storage, &other),
        Err(CommitError::WrongProfile { .. })
    ));
    assert_eq!(
        storage.file(SaveFile::Current),
        before.file(SaveFile::Current)
    );
}

/// AC04 at schema level: future major, oversized, malicious and garbage input
/// return errors, never panic, never overwrite.
#[test]
fn accept_f48_a_future_oversized_and_malicious_saves_are_refused() {
    let future = seal("CSSAVE 2.0\nprofile_id=1\nsomething=else\n");
    assert_eq!(
        decode(&future),
        Err(DecodeError::UnsupportedMajor {
            found: SchemaVersion { major: 2, minor: 0 }
        })
    );
    let mut storage = MemoryStorage::new();
    storage.set_file(SaveFile::Current, Some(future.clone()));
    commit(&mut storage, &doc(1)).expect_err("must not overwrite a future save");
    assert_eq!(storage.file(SaveFile::Current), Some(future.as_slice()));
    storage.set_file(SaveFile::Backup, Some(encode(&doc(1)).expect("e")));
    assert!(matches!(
        recover(&storage),
        Err(RecoverError::UnsupportedMajor {
            file: SaveFile::Current,
            ..
        })
    ));

    assert!(matches!(
        decode(&vec![b'a'; MAX_SAVE_BYTES + 1]),
        Err(DecodeError::TooLarge { .. })
    ));

    let base = "CSSAVE 1.0\nprofile_id=1\nkind=synthetic\ndisplay_name=x\nrevision=1\n";
    for hostile in [
        format!("{base}../../etc/passwd=1\n"),
        format!("{base}record.a/b=1\n"),
        format!("{base}record..hidden=1\n"),
        format!("{base}record.k=18446744073709551616\n"),
        format!("{base}blueprint=../x\n"),
        format!("{base}setting.k=bogus:v\n"),
        format!("{base}profile_id=2\n"),
        format!("{base}no equals sign\n"),
        format!("{base}record.k=1\nrecord.k=2\n"),
        format!("{base}future.k=\u{7}bell\n"),
    ] {
        let sealed = seal(&hostile);
        assert!(decode(&sealed).is_err(), "accepted: {hostile:?}");
    }
    let lines: String = (0..5000).map(|i| format!("future.f{i}=1\n")).collect();
    assert!(decode(&seal(&format!("{base}{lines}"))).is_err());

    for garbage in [
        &b""[..],
        b"\xff\xfe",
        b"CSSAVE",
        b"CSSAVE 1.0\n",
        b"CSSAVE 1.x\nchecksum=0000000000000000\n",
        b"CSSAVE 99999999999.0\n",
    ] {
        assert!(decode(garbage).is_err());
    }
    // Every truncation of a valid file is refused, never accepted or a panic.
    let valid = encode(&doc(7)).expect("e");
    for cut in 0..valid.len() {
        assert!(decode(&valid[..cut]).is_err(), "cut {cut}");
    }
}

#[test]
fn accept_f48_a_unknown_same_major_fields_survive_a_rewrite() {
    let mut d = doc(1);
    d.schema = SchemaVersion { major: 1, minor: 7 };
    d.extra.push(ExtraField {
        key: "future.widget".into(),
        value: "a=b c".into(),
    });
    let again = decode(&encode(&d).expect("e")).expect("d");
    assert_eq!(again.extra, d.extra);
    assert_eq!(again.schema, d.schema);
    assert_eq!(encode(&again).expect("e"), encode(&d).expect("e"));
}

#[test]
fn accept_f48_a_encoder_refuses_out_of_range_documents() {
    let mut d = doc(1);
    d.display_name = "line\nbreak".into();
    assert!(encode(&d).is_err());
    let mut d = doc(1);
    d.records.push(d.records[0].clone());
    assert!(encode(&d).is_err());
    let mut d = doc(1);
    d.settings[0].key = "../x".into();
    assert!(encode(&d).is_err());
    let mut d = doc(1);
    d.display_name = "x".repeat(65);
    assert!(encode(&d).is_err());
}

/// AC03 (schema level): ids come from a high-water mark and are never reused;
/// the display name is not identity.
#[test]
fn accept_f48_a_deleted_profile_ids_are_never_reissued() {
    let mut registry = ProfileRegistry::new();
    let a = registry.allocate().expect("a");
    let b = registry.allocate().expect("b");
    registry.delete(b).expect("delete highest");
    let c = registry.allocate().expect("c");
    assert!(c > b && c != b && c != a);
    assert_eq!(
        registry.delete(b),
        Err(cs_types::profile::RegistryError::UnknownProfile(b))
    );

    // The mark survives a persistence round trip of its parts.
    let restored = ProfileRegistry::from_parts(
        registry.high_water(),
        registry.live().to_vec(),
        registry.active(),
    )
    .expect("parts");
    let mut restored = restored;
    assert!(restored.allocate().expect("d") > c);

    assert!(ProfileRegistry::from_parts(1, vec![pid(5)], None).is_err());
    assert!(ProfileRegistry::from_parts(5, vec![pid(5), pid(5)], None).is_err());
    // A dangling active pointer is not trusted.
    let r = ProfileRegistry::from_parts(5, vec![pid(2)], Some(pid(5))).expect("r");
    assert_eq!(r.active(), None);

    // Two profiles with the same display name keep distinct identities.
    let x = ProfileDocument::synthetic(a, Revision(1));
    let y = ProfileDocument::synthetic(c, Revision(1));
    assert_eq!(x.display_name, y.display_name);
    assert_ne!(x.profile_id, y.profile_id);
    assert_eq!(x.kind, ProfileKind::Synthetic);
}
