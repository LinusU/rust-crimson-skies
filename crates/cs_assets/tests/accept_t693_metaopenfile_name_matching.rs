//! `MetaOpenFile`'s name-matching rule, pinned where it meets retail data
//! (task #693, `docs/findings/2026-10-06-t693-metaopenfile-name-matching.md`).
//!
//! The rule itself is read out of `GOSDATA/ASSETS/BINARIES/roffile.dll` — a
//! container request is upper-cased and compared byte for byte against the
//! name the container stores, a loose request reaches `CreateFileA` unfolded
//! — and that reading is code-derived, not a run of the original engine, so
//! no test here can prove it. What a test *can* pin is what the rule then
//! implies on this installation, measured through production code only:
//!
//! * every name both containers store is ASCII-uppercase, which is the
//!   premise under which the original's rule and `GosNameMatch::AsciiInsensitive`
//!   answer a request the same way;
//! * exactly two paths exist on both sides of the chain and differ only in
//!   case — the `ARIAL8.TGA` / `FONT.TGA` pair #686 named, and nothing else;
//! * the lowercase request is answered by **`crimson.rof`**, the source
//!   registered before the loose directory, under every casing of the
//!   request;
//! * and the container's decoded member is **byte-identical** to the loose
//!   file, so for these two names the rule decides which source reports the
//!   answer, not which bytes arrive.
//!
//! The test reads `$CS_GAME_DIR` read-only and fails loudly without it; CI
//! skips it.

/// The acceptance prefix of this task.
const _PREFIX: &str = "accept_t693_";

use std::collections::BTreeMap;
use std::path::PathBuf;

use cs_assets::install::{self, fingerprint, sha256};
use cs_assets::vfs::{
    ContentSession, ExePathOrigin, GosChain, GosInstall, GosNameMatch, GosSource, MAIN_MOUNT_ID,
    SessionBuilder, gos_key,
};
use cs_types::asset_id::ResolveContext;

/// The container spelling of the two case-only pairs, and the loose tree's own
/// spelling of each, as #341's section D and #686's section C recorded them.
const CASE_ONLY_PAIRS: [(&str, &str); 2] = [
    ("ASSETS/GRAPHICS/ARIAL8.TGA", "ASSETS/GRAPHICS/arial8.tga"),
    ("ASSETS/GRAPHICS/FONT.TGA", "ASSETS/GRAPHICS/font.tga"),
];

/// The decoded bytes of each pair member, measured on this installation.
const PAIR_LENGTHS: [usize; 2] = [45_636, 65_580];

/// The sha256 of each pair's bytes — the same digest on both sides of the
/// pair, which is what makes the rule cosmetic for content here.
const PAIR_SHA256: [&str; 2] = ["a8d6dca7", "3c544ab4"];

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var_os("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

/// One mounted chain plus the session its sources are read through.
struct Chain {
    session: ContentSession,
    chain: GosChain,
}

/// Mounts the installation's GOS chain through the production builder,
/// stating no rule, so the default the chain answers with is the one under
/// test.
fn mount(root: &std::path::Path) -> Chain {
    let found = install::discover(root).expect("the installation is discovered");
    let context = ResolveContext::new(fingerprint(&found.manifest));
    let install = GosInstall::new(root, ExePathOrigin::inspect_registry(root));
    let mut builder = SessionBuilder::new(context);
    let chain = builder.mount_gos_chain(&install).expect("the chain mounts");
    let session = builder.open();
    assert!(
        session.rejected().is_empty(),
        "no member of this installation is refused: {:?}",
        session.rejected()
    );
    Chain { session, chain }
}

/// Every member a chain registered for `source`, keyed by its logical
/// (case-folded) path and holding its own spelling and its mounted digest.
fn members_of(chain: &Chain, source: GosSource) -> BTreeMap<String, (String, u64, String)> {
    let step = chain
        .chain
        .step_of(source)
        .unwrap_or_else(|| panic!("{} is registered", source.label()));
    let mut members = BTreeMap::new();
    for attempt in chain.session.mounts() {
        if attempt.id() != &step.mount {
            continue;
        }
        for (_, member) in attempt.members() {
            members.insert(
                member.spelling().logical_key().to_owned(),
                (
                    member.spelling().as_str().to_owned(),
                    member.size_bytes(),
                    member
                        .sha256()
                        .unwrap_or_else(|| panic!("{} is hashed", member.spelling().as_str()))
                        .to_hex(),
                ),
            );
        }
    }
    assert_eq!(
        members.len(),
        step.members,
        "{} contributes exactly what its step counted",
        source.label()
    );
    members
}

/// On the original installation: the premise under which the rule matters,
/// the pairs it decides, and the bytes each pair then serves.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t693_metaopenfile_name_matching_retail_pairs_and_container_spelling() {
    let root = game_dir();
    let chain = mount(&root);

    // The rule the chain answers with, un-stated by the caller: #686's
    // documented default. #693 reads the original as agreeing with it on
    // case for this installation's containers, below.
    assert_eq!(chain.chain.name_match(), GosNameMatch::AsciiInsensitive);

    // Premise: no name either container stores holds a lowercase letter, so
    // `upper(request) == stored` and an ASCII case-insensitive request match
    // the same members. Directory components are included, because they are
    // entries of a directory block like any other.
    let patch = members_of(&chain, GosSource::PatchContainer);
    let main = members_of(&chain, GosSource::MainContainer);
    assert_eq!(patch.len(), 1, "crimptch.rof's single member");
    assert_eq!(main.len(), 846, "crimson.rof's members");
    for (label, members) in [("patch", &patch), ("main", &main)] {
        for (key, (spelling, ..)) in members {
            assert!(
                !spelling.chars().any(|c| c.is_ascii_lowercase()),
                "{label} member {key} is spelled {spelling}, with a lowercase letter"
            );
            assert_eq!(
                &key.to_ascii_uppercase(),
                spelling,
                "{label} member {key} folds to exactly its own spelling"
            );
        }
    }

    // The loose tree, keyed the same way: what exists on both sides of the
    // chain at all, ignoring case.
    let loose = members_of(&chain, GosSource::LooseUiAssets);
    assert_eq!(loose.len(), 18, "the loose GOSDATA tree's files");
    let both_sides: Vec<String> = loose
        .keys()
        .filter(|key| main.contains_key(*key))
        .cloned()
        .collect();
    let expected: Vec<String> = CASE_ONLY_PAIRS
        .iter()
        .map(|(_, loose_spelling)| loose_spelling.to_ascii_lowercase())
        .collect();
    assert_eq!(
        both_sides, expected,
        "exactly the two case-only pairs exist on both sides, nothing else"
    );

    // Each pair really is a case-only difference of spelling, on both sides.
    for (index, pair) in CASE_ONLY_PAIRS.iter().enumerate() {
        let (container_spelling, loose_spelling) = *pair;
        let key = loose_spelling.to_ascii_lowercase();
        let (container_record, ..) = main
            .get(&key)
            .unwrap_or_else(|| panic!("{container_spelling} is in crimson.rof"));
        let (loose_record, ..) = loose.get(&key).unwrap_or_else(|| panic!("it is loose too"));
        assert_eq!(container_record, container_spelling);
        assert_eq!(loose_record, loose_spelling);
        assert_ne!(
            container_record, loose_record,
            "the pair differs only in case"
        );
        assert_eq!(
            container_record.to_ascii_lowercase(),
            loose_record.to_ascii_lowercase(),
            "and in nothing else"
        );

        // The answer the original's rule gives: the container is registered
        // first and it matches under every casing of the request, so the
        // container serves — and the bytes it serves are the loose file's.
        for request in [container_spelling, loose_spelling] {
            let served = chain
                .session
                .resolve(&gos_key(request).expect("a valid key"))
                .unwrap_or_else(|error| panic!("{request} resolves: {error:?}"));
            assert_eq!(
                served.resolved().mount.as_str(),
                MAIN_MOUNT_ID,
                "{request} is answered by crimson.rof, not the loose copy"
            );
            assert_eq!(
                served.resolved().span.member_key(),
                Some(container_spelling),
                "{request} is answered by the container's own spelling"
            );
            let served_bytes = chain
                .chain
                .read(&chain.session, &served)
                .expect("the container member reads through the production reader");
            let loose_bytes = std::fs::read(
                root.join("GOSDATA")
                    .join(loose_spelling.replace('/', std::path::MAIN_SEPARATOR_STR)),
            )
            .unwrap_or_else(|error| panic!("{loose_spelling} is readable: {error}"));
            assert_eq!(
                served_bytes, loose_bytes,
                "{container_spelling} and its loose copy are the same bytes"
            );
            assert_eq!(
                served_bytes.len(),
                PAIR_LENGTHS[index],
                "{container_spelling} decoded length"
            );
            let digest = sha256(&served_bytes).to_hex();
            assert!(
                digest.starts_with(PAIR_SHA256[index]),
                "{container_spelling} digest {digest}"
            );
        }
    }
}
