//! F61-A acceptance scenarios: the release-contents policy.
//! Task test prefix: `accept_f61_a_`.
//!
//! Spec: `specs/F61-distribution-installation-ux-notices-and-release-artifacts.md`
//! (deliverable, non-negotiable 1–3; acceptance test AC01). The code under
//! test is `cs_xtask::package`, the production policy; this file adds no
//! classification of its own except where a case is spelled out explicitly so
//! that *removing* a policy entry fails a test rather than silently widening
//! what a release may carry.
//!
//! Every fixture is newly authored synthetic data. The committed candidate
//! manifest under `packaging/fixtures/` holds text written for this
//! repository; the negative cases add members to a copy of it in memory. The
//! one fixture written to disk by this file is a manifest under the gitignored
//! `target/`, so nothing lands in Git. No test opens `$CS_GAME_DIR`, and no
//! original content is present or read.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use cs_xtask::package::{
    CandidatePackage, Finding, MemberClass, NoticeKind, PROPRIETARY_SUFFIXES, PackageMember,
    ProprietaryKind, REQUIRED_NOTICES, ReleasePolicy, UnsafePath, classify, parse_manifest,
    read_manifest, scan, unsafe_path,
};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The committed clean candidate, read from disk through the production parser.
fn clean_candidate() -> CandidatePackage {
    read_manifest(&workspace_root().join("packaging/fixtures/candidate-clean.manifest"))
        .expect("the committed clean candidate manifest must parse")
}

/// A copy of the clean candidate with `extra` members appended.
fn with_members(extra: &[&str]) -> CandidatePackage {
    let mut candidate = clean_candidate();
    candidate
        .members
        .extend(extra.iter().map(|path| PackageMember::new(*path, 1024)));
    candidate
}

/// A copy of the clean candidate with every member whose path is `dropped`
/// removed.
fn without_members(dropped: &[&str]) -> CandidatePackage {
    let mut candidate = clean_candidate();
    candidate
        .members
        .retain(|member| !dropped.contains(&member.path.as_str()));
    candidate
}

/// The findings that block a candidate, as the pairs a report names them by.
fn blocking(candidate: &CandidatePackage) -> Vec<(String, String)> {
    scan(candidate)
        .findings
        .iter()
        .map(|finding| (format!("{finding:?}"), finding.to_string()))
        .collect()
}

/// The committed fixture is the shape a release is allowed to have: the engine,
/// every required notice, documentation, and hash manifests — and nothing else.
///
/// Observable failure if the policy is removed or hollowed out: the fixture is
/// read from the repository with the production parser and scanned with the
/// production scan, so a classifier that returned "allowed" for everything, or
/// a parser that returned no members, makes this fail rather than pass.
#[test]
fn accept_f61_a_the_committed_candidate_is_releasable() {
    let candidate = clean_candidate();
    assert_eq!(candidate.version, "0.1.0", "the fixture states its version");
    assert_eq!(
        candidate.members.len(),
        11,
        "the fixture lost members; a scan of an empty package would pass for the wrong reason"
    );

    let report = scan(&candidate);
    assert!(
        report.is_releasable(),
        "the committed candidate must be releasable, got: {:#?}",
        report.lines()
    );
    assert_eq!(report.member_count, candidate.members.len());
    assert_eq!(
        report.total_bytes,
        candidate.members.iter().map(|m| m.size_bytes).sum::<u64>()
    );

    // Each member is in the class the policy claims for it, not merely allowed.
    assert_eq!(
        classify("crimson-skies"),
        MemberClass::EngineBinary,
        "the engine executable must be recognised as such"
    );
    for (notice, path) in REQUIRED_NOTICES {
        assert_eq!(
            classify(path),
            MemberClass::Notice(notice),
            "{path} must satisfy the {notice} requirement"
        );
    }
    assert_eq!(classify("docs/installation.md"), MemberClass::Documentation);
    assert_eq!(
        classify("content-sha256.txt.sha256"),
        MemberClass::HashManifest
    );
    assert_eq!(classify("SHA256SUMS"), MemberClass::HashManifest);
}

/// AC01, first half: a candidate carrying proprietary content is refused, and
/// the finding names the member and the class of content.
///
/// The first loop is coverage — every entry of the production table is
/// exercised, so removing one leaves the remaining loop shorter and the
/// explicit cases below fail. The explicit list is what makes that removal a
/// test failure rather than a silent shrink.
#[test]
fn accept_f61_a_proprietary_content_is_refused() {
    for (suffix, kind) in PROPRIETARY_SUFFIXES {
        let path = format!("assets/payload{suffix}");
        assert_eq!(
            classify(&path),
            MemberClass::Proprietary(*kind),
            "{path} must classify as {kind}"
        );
        assert!(
            !MemberClass::Proprietary(*kind).is_allowed(),
            "{kind} must not be shippable"
        );
    }

    // Spelled out, so the table cannot quietly lose one of these.
    const CASES: [(&str, ProprietaryKind); 10] = [
        ("textures/plane00.dds", ProprietaryKind::Texture),
        ("sounds/eng_01.wav", ProprietaryKind::Audio),
        ("scripts/m01.win.lua", ProprietaryKind::Script),
        ("fonts/arial.ttf", ProprietaryKind::Font),
        ("setup.exe", ProprietaryKind::Executable),
        ("manual/crs-manual.pdf", ProprietaryKind::ManualScan),
        ("data/planes.zip", ProprietaryKind::ExtractedArchive),
        ("decomp/planes.asm", ProprietaryKind::DecompiledSource),
        (
            "original/Textures/plane00.dat",
            ProprietaryKind::OriginalContentTree,
        ),
        (
            "extracted/Data/Sounds/eng_01.bin",
            ProprietaryKind::OriginalContentTree,
        ),
    ];
    for (path, kind) in CASES {
        assert_eq!(
            classify(path),
            MemberClass::Proprietary(kind),
            "{path} must classify as {kind}"
        );
        let report = scan(&with_members(&[path]));
        assert!(
            !report.is_releasable(),
            "a candidate carrying {path} must not be releasable"
        );
        assert!(
            report.findings.contains(&Finding::ProprietaryContent {
                path: path.to_string(),
                kind,
            }),
            "the report must name {path} as {kind}, got: {:#?}",
            report.lines()
        );
    }

    // A new class of proprietary content has to arrive with a case here: the
    // loop above walks the suffix table, but a variant with no suffix — or one
    // only an original-content root catches — would otherwise be covered by
    // nothing at all.
    let covered: BTreeSet<ProprietaryKind> = CASES.iter().map(|(_, kind)| *kind).collect();
    let every: BTreeSet<ProprietaryKind> = ProprietaryKind::ALL.into_iter().collect();
    assert_eq!(
        covered, every,
        "every ProprietaryKind needs a spelled-out case in this test"
    );

    // Case does not smuggle content past the table.
    assert_eq!(
        classify("TEXTURES/Plane00.DDS"),
        MemberClass::Proprietary(ProprietaryKind::Texture)
    );
    assert_eq!(
        classify("Setup.EXE"),
        MemberClass::Proprietary(ProprietaryKind::Executable)
    );
}

/// A policy that only lists what it refuses would still let every other file
/// through. The engine ships binaries; the *only* executable a release may
/// carry is the engine, and a file that merely sits in a directory named after
/// the engine does not stand in for it.
#[test]
fn accept_f61_a_only_the_declared_engine_executable_may_ship() {
    assert_eq!(classify("bin/crimson-skies.exe"), MemberClass::EngineBinary);
    assert_eq!(
        classify("CRIMSON-SKIES"),
        MemberClass::EngineBinary,
        "the engine name is matched case-insensitively"
    );
    assert!(scan(&clean_candidate()).is_releasable());

    // Any other executable is proprietary content.
    for path in [
        "helpers/launcher.exe",
        "crimson-skies-helper.dll",
        "vcredist.msi",
    ] {
        let report = scan(&with_members(&[path]));
        assert!(
            !report.is_releasable(),
            "{path} is not the engine and may not ship"
        );
    }

    // A documentation file named after the engine must not satisfy "this
    // release ships an engine" (non-negotiable 4's delivery half).
    assert_eq!(
        classify("docs/crimson-skies"),
        MemberClass::Unclassified,
        "a file named after the engine outside the root or bin/ is not the engine"
    );
    assert_eq!(
        classify(r"docs\crimson-skies"),
        MemberClass::Unclassified,
        "the engine-directory rule must not depend on how the member spelled its separators"
    );
    assert_eq!(
        classify(r"original\data\plane00.dat"),
        MemberClass::Proprietary(ProprietaryKind::OriginalContentTree),
        "an original-content root spelled with backslashes is still an original-content root"
    );
    let mut candidate = clean_candidate();
    candidate
        .members
        .retain(|member| member.path != "crimson-skies");
    for impostor in [
        "docs/crimson-skies",
        r"docs\crimson-skies",
        r"bin\..\docs\crimson-skies",
    ] {
        let mut candidate = candidate.clone();
        candidate.members.push(PackageMember::new(impostor, 512));
        let report = scan(&candidate);
        assert!(!report.is_releasable(), "no engine, so not releasable");
        assert!(
            report.findings.contains(&Finding::MissingEngineBinary),
            "the report must say the engine is missing, got: {:#?}",
            report.lines()
        );
    }
}

/// AC01, second half: a candidate missing a required notice is refused, and one
/// missing every notice and the engine is refused with all of them named.
#[test]
fn accept_f61_a_a_missing_notice_is_refused() {
    for (notice, path) in REQUIRED_NOTICES {
        let report = scan(&without_members(&[path]));
        assert!(
            !report.is_releasable(),
            "a candidate without {path} must not be releasable"
        );
        assert!(
            report.findings.contains(&Finding::MissingNotice {
                kind: notice,
                expected: path,
            }),
            "the report must name the missing {notice}, got: {:#?}",
            report.lines()
        );
        assert!(
            report
                .lines()
                .iter()
                .any(|line| line.contains(notice.label())),
            "the failure text must name the missing notice, got: {:#?}",
            report.lines()
        );
    }

    let empty = CandidatePackage::new("0.1.0", Vec::new());
    let report = scan(&empty);
    assert!(!report.is_releasable());
    let missing = report
        .findings
        .iter()
        .filter(|finding| {
            matches!(
                finding,
                Finding::MissingNotice { .. } | Finding::MissingEngineBinary
            )
        })
        .count();
    assert_eq!(
        missing,
        REQUIRED_NOTICES.len() + 1,
        "an empty candidate must be missing every notice and the engine, got: {:#?}",
        report.lines()
    );
    assert_eq!(report.member_count, 0);
}

/// Non-negotiable 1's explicit exception: a source hash manifest is not an asset
/// bundle. A release states which original content it was verified against
/// without carrying any of it — and the same rule must still bite inside an
/// original-content root, or the exception launders content.
#[test]
fn accept_f61_a_a_source_hash_manifest_is_not_an_asset_bundle() {
    for path in [
        "verified-content.sha256",
        "release.sha512",
        "release.sha1",
        "original-content-sha256.sha512",
        "docs/hashes.md5",
    ] {
        assert_eq!(
            classify(path),
            MemberClass::HashManifest,
            "{path} must be recognised as a hash manifest, not as an asset bundle"
        );
        assert!(
            scan(&with_members(&[path])).is_releasable(),
            "a release may carry {path}"
        );
    }

    // But the exception does not extend into an original-content root, and a
    // hash manifest is not a licence to ship the file it names.
    let report = scan(&with_members(&["original/content-sha256.txt.sha256"]));
    assert!(!report.is_releasable());
    assert!(
        report.findings.contains(&Finding::ProprietaryContent {
            path: "original/content-sha256.txt.sha256".to_string(),
            kind: ProprietaryKind::OriginalContentTree,
        }),
        "a hash manifest under original/ must still be refused, got: {:#?}",
        report.lines()
    );
}

/// Unknown means unknown: a member nothing in the policy classifies is refused,
/// and a member path that could escape the archive is refused before it is
/// classified at all.
#[test]
fn accept_f61_a_unclassified_and_unsafe_members_are_refused() {
    for path in ["assets/payload.bin", "data/tables.dat", "run.sh"] {
        assert_eq!(
            classify(path),
            MemberClass::Unclassified,
            "{path} has no class in the policy"
        );
        let report = scan(&with_members(&[path]));
        assert!(!report.is_releasable(), "{path} must be refused");
        assert!(
            report.findings.contains(&Finding::UnclassifiedMember {
                path: path.to_string()
            }),
            "the report must name {path} as unclassified, got: {:#?}",
            report.lines()
        );
    }

    for (path, reason) in [
        ("../escape.txt", UnsafePath::ParentTraversal),
        ("docs/../../escape.txt", UnsafePath::ParentTraversal),
        ("/absolute.txt", UnsafePath::Absolute),
        ("C:\\windows\\escape.txt", UnsafePath::Absolute),
        ("./docs/installation.md", UnsafePath::NotCanonical),
        ("docs//installation.md", UnsafePath::NotCanonical),
        ("", UnsafePath::Empty),
    ] {
        assert_eq!(
            unsafe_path(path),
            Some(reason),
            "{path:?} must be refused as {reason:?}"
        );
        let report = scan(&with_members(&[path]));
        assert!(!report.is_releasable(), "{path:?} must be refused");
        assert!(
            report.findings.contains(&Finding::UnsafeMemberPath {
                path: path.to_string(),
                reason,
            }),
            "the report must name {path:?}, got: {:#?}",
            report.lines()
        );
    }

    assert_eq!(unsafe_path("docs/installation.md"), None);

    // Two spellings of one member are a finding, not a silent overwrite.
    let mut candidate = clean_candidate();
    candidate
        .members
        .push(PackageMember::new("docs/installation.md", 999));
    let report = scan(&candidate);
    assert!(
        report.findings.contains(&Finding::DuplicateMember {
            path: "docs/installation.md".to_string()
        }),
        "a repeated member must be refused, got: {:#?}",
        report.lines()
    );
}

/// A manifest is the packaging input, and a broken one is a failure rather than
/// an empty candidate that passes.
#[test]
fn accept_f61_a_a_broken_candidate_manifest_is_rejected() {
    assert!(parse_manifest("m", "# only comments\n\n").is_err());
    assert!(
        parse_manifest("m", "version: 0.1.0\nmember crimson-skies\n")
            .expect_err("a member without a size must be rejected")
            .to_string()
            .contains("must be `member <path> <size-bytes>`"),
        "the error must say what a member line looks like"
    );
    assert!(
        parse_manifest("m", "version: 0.1.0\nmember docs/my notes.md 12\n").is_err(),
        "a member path with whitespace must be rejected, not split"
    );
    assert!(
        parse_manifest("m", "version: 0.1.0\nmember crimson-skies huge\n").is_err(),
        "a size that is not a number must be rejected"
    );
    assert!(
        parse_manifest("m", "member crimson-skies 12\n")
            .expect_err("an unversioned manifest must be rejected")
            .to_string()
            .contains("states no `version:` line"),
        "the error must name the missing version"
    );
    assert!(
        parse_manifest("m", "version: 0.1 0\n").is_err(),
        "a version holding whitespace must be rejected"
    );
    assert!(
        parse_manifest("m", "package: whatever\n").is_err(),
        "an unknown directive must be rejected"
    );

    // A rejected manifest is a rejected manifest: it is an error, never an
    // empty package that then scans clean.
    let error = read_manifest(&workspace_root().join("packaging/fixtures/does-not-exist.manifest"))
        .expect_err("a missing manifest must be an error");
    assert!(
        error.to_string().contains("cannot read candidate manifest"),
        "the error must say what failed, got: {error}"
    );
    assert!(
        !blocking(&CandidatePackage::new("0.1.0", Vec::new())).is_empty(),
        "the only way an empty candidate is acceptable is never"
    );
}

/// The policy is a value, so a different distribution can state its own
/// requirements without editing the release rules the other tests pin.
#[test]
fn accept_f61_a_the_policy_is_a_value_a_later_stage_can_replace() {
    let internal = ReleasePolicy {
        required_notices: vec![(NoticeKind::License, "LICENSE")],
        engine_binaries: vec!["internal-build".to_string()],
    };
    let candidate = CandidatePackage::new(
        "0.1.0",
        vec![
            PackageMember::new("internal-build", 10),
            PackageMember::new("LICENSE", 10),
        ],
    );
    assert!(
        internal.scan(&candidate).is_releasable(),
        "a policy that asks for less must accept less"
    );
    assert!(
        !scan(&candidate).is_releasable(),
        "the release policy still asks for every notice and the engine"
    );
    assert_eq!(
        internal.required_notices.len(),
        1,
        "an internal policy asks for less than the release policy"
    );
    assert_eq!(
        ReleasePolicy::release().required_notices.len(),
        REQUIRED_NOTICES.len(),
        "the release policy must ask for every notice the spec names"
    );
    assert_eq!(
        ReleasePolicy::release().engine_binaries,
        vec!["crimson-skies".to_string(), "crimson-skies.exe".to_string()],
        "the release policy ships the new engine and nothing else executable"
    );
}

/// The sizes in a candidate manifest are declared in a text file, so the report
/// must survive an absurd one: a gate that panics or wraps on hostile input is
/// not a gate that refused anything.
///
/// Observable failure: with an overflowing sum this test fails inside the scan
/// (a debug-build overflow panic), and a wrapping sum reports a total no member
/// of the candidate declares.
#[test]
fn accept_f61_a_an_absurd_declared_size_is_scanned_not_fatal() {
    let mut absurd = clean_candidate();
    for member in &mut absurd.members {
        member.size_bytes = u64::MAX;
    }
    let report = scan(&absurd);
    assert!(
        report.is_releasable(),
        "the declared sizes do not change the shape of a candidate, got: {:#?}",
        report.lines()
    );
    assert_eq!(
        report.total_bytes,
        u64::MAX,
        "a total past u64::MAX saturates instead of wrapping or panicking"
    );

    // And the scan keeps working on the member list around it.
    let report = scan(&with_members(&["textures/plane00.dds"]));
    assert_eq!(
        report.findings,
        vec![Finding::ProprietaryContent {
            path: "textures/plane00.dds".to_string(),
            kind: ProprietaryKind::Texture,
        }],
        "a proprietary member is still the only finding"
    );
}

/// The gate an agent or a packaging step actually runs: `cs_xtask
/// verify-package --manifest <file>` exits 0 for a releasable candidate, exits 1
/// with every finding on stderr for one that may not ship, and exits 2 when the
/// request itself is wrong. A subcommand that printed success while refusing
/// nothing would satisfy the library tests and still let a bad release out.
///
/// The scratch manifests live under the gitignored `target/`, so nothing this
/// test writes lands in Git or in the packaging inputs.
#[test]
fn accept_f61_a_the_verify_package_command_fails_a_release_it_may_not_ship() {
    let root = workspace_root();
    let bin = env!("CARGO_BIN_EXE_cs_xtask");
    let fixtures = root.join(format!(
        "target/f61-a-package-fixtures/{}",
        std::process::id()
    ));
    fs::create_dir_all(&fixtures).expect("the scratch fixture directory must be creatable");

    let run = |args: &[&std::ffi::OsStr]| {
        cs_xtask::transient::command_output(Command::new(bin).args(args))
            .expect("the cs_xtask binary must run")
    };
    fn as_os(text: &str) -> &std::ffi::OsStr {
        std::ffi::OsStr::new(text)
    }

    // The committed candidate passes, and says so on stdout.
    let passing = run(&[
        as_os("verify-package"),
        as_os("--workspace-root"),
        root.as_os_str(),
        as_os("--manifest"),
        root.join("packaging/fixtures/candidate-clean.manifest")
            .as_os_str(),
    ]);
    assert_eq!(
        passing.status.code(),
        Some(0),
        "verify-package must pass the committed candidate; stderr: {}",
        String::from_utf8_lossy(&passing.stderr)
    );
    assert!(
        String::from_utf8_lossy(&passing.stdout).contains("is releasable"),
        "a pass must say what passed, got: {:?}",
        String::from_utf8_lossy(&passing.stdout)
    );

    // Proprietary content and five missing notices: exit 1, every finding named.
    let bad = fixtures.join("candidate-proprietary.manifest");
    fs::write(
        &bad,
        "version: 0.1.0\n\
         member crimson-skies 12582912\n\
         member LICENSE 1073\n\
         member textures/plane00.dds 4096\n",
    )
    .expect("the negative candidate must be writable");
    let failing = run(&[
        as_os("verify-package"),
        as_os("--workspace-root"),
        root.as_os_str(),
        as_os("--manifest"),
        bad.as_os_str(),
    ]);
    assert_eq!(
        failing.status.code(),
        Some(1),
        "a candidate carrying proprietary content must fail the gate, not print success"
    );
    let stderr = String::from_utf8_lossy(&failing.stderr);
    for expected in [
        "textures/plane00.dds",
        "texture",
        "NOTICE.md",
        "COMPATIBILITY.md",
    ] {
        assert!(
            stderr.contains(expected),
            "the failure must name {expected}, got: {stderr:?}"
        );
    }

    // A manifest that cannot be read is a failure, never an empty candidate that
    // passes for the wrong reason.
    let unreadable = run(&[
        as_os("verify-package"),
        as_os("--workspace-root"),
        root.as_os_str(),
        as_os("--manifest"),
        fixtures.join("does-not-exist.manifest").as_os_str(),
    ]);
    assert_eq!(
        unreadable.status.code(),
        Some(1),
        "a missing manifest must fail the gate"
    );
    assert!(
        String::from_utf8_lossy(&unreadable.stderr).contains("cannot read candidate manifest"),
        "the failure must say what failed, got: {:?}",
        String::from_utf8_lossy(&unreadable.stderr)
    );

    // The request itself being wrong is a usage error, and an unusable
    // --workspace-root is a failure rather than a flag that is quietly ignored.
    // The releasable candidate is the one that proves the second half: against a
    // root that is not a workspace it must not report success.
    assert_eq!(
        run(&[as_os("verify-package")]).status.code(),
        Some(2),
        "verify-package without --manifest must be a usage error"
    );
    assert_eq!(
        run(&[
            as_os("verify-package"),
            as_os("--workspace-root"),
            fixtures.as_os_str(),
            as_os("--manifest"),
            root.join("packaging/fixtures/candidate-clean.manifest")
                .as_os_str(),
        ])
        .status
        .code(),
        Some(1),
        "a --workspace-root that is not a workspace must fail, not be ignored"
    );

    let _ = fs::remove_dir_all(&fixtures);
}
