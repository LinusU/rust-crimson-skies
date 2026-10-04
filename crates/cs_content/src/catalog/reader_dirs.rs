//! Classifies the reader-archive directories the campaign walk leaves over
//! (F14-D.1).
//!
//! F14-D recorded every `zrdr.zbd` the campaign layout does not claim as an
//! unknown. This module decides what each of them is **from the archive's own
//! member index**, and refuses to decide when the evidence is not there: a
//! directory whose archive cannot be listed, or whose members fit none of the
//! rules below, stays unclassified and is reported as such.
//!
//! The observations behind the rules were measured on the owner's installation
//! (`docs/findings/2026-10-02-f14-d-1-reader-archive-directories.md`):
//!
//! * a **scenario directory** (`ZBD/<world group>/<leaf>`) holds the same two
//!   files a campaign mission directory holds (`zrdr.zbd` and `mis_anim.zbd`)
//!   and its reader lists the per-mission members (`map.zrd`, `aiv.zrd`,
//!   `objectives.zrd`). A leaf named `IA<n>` whose reader also lists `ia.zrd`
//!   is an instant-action scenario directory; a leaf named `MP<n>` whose
//!   reader also lists `net.zrd` is a multiplayer scenario directory. A name
//!   its own archive does not corroborate is never counted;
//! * **what the corroboration is worth differs between the two roles**, and
//!   the rules below do not hide that. `ia.zrd` occurs in no other reader on
//!   the owner's installation, so it decides instant action on its own.
//!   `net.zrd` occurs in the campaign-mission readers too, so it marks a
//!   networked reader and rules instant action *out*; what separates a
//!   multiplayer scenario from a campaign mission is the directory name, and
//!   that is measured independently by F56-A
//!   (`docs/findings/2026-10-02-f56-a-multiplayer-catalog.md`, the same 21
//!   `MP<n>` slots). A leaf named `M<nn>` is therefore never classified as a
//!   scenario, however its reader is shaped;
//! * a group is only a group because the campaign walk declares it, so a
//!   scenario directory under a group the layout does not declare stays
//!   unknown rather than being counted (world rows are F14-D.2 / #389);
//! * a **world-group reader** (`ZBD/<world group>/zrdr.zbd`) lists the shared
//!   world members (`templates.zrd`, `cam_anim.zrd`) and none of the
//!   per-mission ones, and has no `mis_anim.zbd` beside it;
//! * the **shared reader** (`ZBD/zrdr.zbd`) lists the install-wide definitions
//!   (`instantaction.zrd`, `multiplayer_setup.zrd`) and none of the
//!   per-mission members.
//!
//! These are structural facts about the archives, not an interpretation of
//! their records: no record is decoded here (F49/F56/F18 own that), and a
//! classification as a scenario says the directory is shaped like a scenario
//! the game can start, never that it plays.

use std::collections::BTreeSet;

/// What one reader-archive directory is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReaderDirRole {
    /// An instant-action scenario directory (`ZBD/<world group>/IA<n>`).
    InstantActionScenario,
    /// A multiplayer scenario directory (`ZBD/<world group>/MP<n>`).
    MultiplayerScenario,
    /// A world group's shared reader (`ZBD/<world group>/zrdr.zbd`): not a
    /// launchable scenario, a dependency of every scenario of the group.
    WorldGroupReader,
    /// The install-wide reader (`ZBD/zrdr.zbd`): not a launchable scenario, a
    /// dependency of every scenario.
    SharedReader,
}

impl ReaderDirRole {
    /// Stable lowercase label for reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::InstantActionScenario => "instant_action_scenario",
            Self::MultiplayerScenario => "multiplayer_scenario",
            Self::WorldGroupReader => "world_group_reader",
            Self::SharedReader => "shared_reader",
        }
    }

    /// Whether the role is a scenario the game can start, and so a row of the
    /// coverage denominator.
    pub const fn is_launchable(self) -> bool {
        matches!(
            self,
            Self::InstantActionScenario | Self::MultiplayerScenario
        )
    }
}

/// One reader-archive directory with its role and the members that decided it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassifiedReaderDir {
    /// The directory's spelling inside the installation.
    pub path: String,
    /// The reader archive it holds, as spelled.
    pub program: String,
    /// The digest of that archive's bytes, taken from the inventory.
    pub program_sha256: String,
    /// What the directory is.
    pub role: ReaderDirRole,
    /// How many distinct lowercase member names the archive's own index
    /// lists. This is the count of distinct names, not the count the archive
    /// declares: the owner's `ZBD/zrdr.zbd` declares 221 members and lists
    /// `player.zrd` twice, so it is 220 here.
    pub members: usize,
    /// The member names (lowercase) that corroborate the role, sorted.
    pub evidence: Vec<&'static str>,
}

/// The members a per-mission reader lists and a shared reader does not.
const MISSION_MEMBERS: [&str; 3] = ["map.zrd", "aiv.zrd", "objectives.zrd"];

/// The members a world-group reader lists and the shared reader does not,
/// sorted, as [`ClassifiedReaderDir::evidence`] publishes them.
const WORLD_GROUP_MEMBERS: [&str; 2] = ["cam_anim.zrd", "templates.zrd"];

/// The members the install-wide reader lists and a world-group reader does not.
const SHARED_READER_MEMBERS: [&str; 2] = ["instantaction.zrd", "multiplayer_setup.zrd"];

/// Classifies an **installation-scope** reader archive — the install-wide
/// `ZBD/zrdr.zbd` or a world group's `ZBD/<world group>/zrdr.zbd` — from the
/// lowercase member names its own index lists, and returns the members that
/// decided it, or `None` when the evidence does not decide.
///
/// This is the one definition of the two scope rules: [`classify`] calls it, so a
/// caller that already knows which archive it holds (F39-E3's installation-scope
/// census walks the non-mission-scoped `zrdr.zbd` archives itself) and the
/// campaign walk that holds no such assumption cannot classify the same archive
/// two different ways.
///
/// The rules are the two the module doc records, measured on the owner's
/// installation: a world-group reader lists the shared world members and none of
/// the per-mission ones, and the install-wide reader lists the install-wide
/// definitions and none of the per-mission ones; neither has a `mis_anim.zbd`
/// beside it. A reader that lists a per-mission member is **not** an
/// installation-scope reader — that is the rule that keeps an objective record
/// out of a scope row (F39-E3's measurement), so the two roles are separated by
/// the members themselves and not by the archive's path.
///
/// `has_mis_anim` is whether a `mis_anim.zbd` sits beside the archive; the
/// campaign walk measures it from the inventory and a direct caller measures it
/// the same way. It is a parameter, not an assumption: a `mis_anim.zbd` beside an
/// archive is F14-D.1's own signal that the directory is a scenario, so one
/// sitting next to a scope archive stops the classification instead of being
/// ignored.
#[must_use]
pub fn classify_installation_scope(
    members: &BTreeSet<String>,
    has_mis_anim: bool,
) -> Option<(ReaderDirRole, Vec<&'static str>)> {
    if has_mis_anim || MISSION_MEMBERS.iter().any(|name| members.contains(*name)) {
        return None;
    }
    if WORLD_GROUP_MEMBERS
        .iter()
        .all(|name| members.contains(*name))
    {
        return Some((
            ReaderDirRole::WorldGroupReader,
            WORLD_GROUP_MEMBERS.to_vec(),
        ));
    }
    if SHARED_READER_MEMBERS
        .iter()
        .all(|name| members.contains(*name))
    {
        return Some((ReaderDirRole::SharedReader, SHARED_READER_MEMBERS.to_vec()));
    }
    None
}

/// Classifies one directory, or returns `None` when the evidence does not
/// decide.
///
/// `path` is the directory spelling (`ZBD/C1/IA1`), `members` the lowercase
/// member names of its reader archive, `has_mis_anim` whether a
/// `mis_anim.zbd` sits beside the archive, and `world_groups` the lowercase
/// world-group directory names the campaign layout declares.
///
/// The leaf name carries the role, and the members keep a directory whose
/// name says something its own archive does not corroborate out of the
/// denominator. `ia.zrd` is decisive on its own; `net.zrd` only proves the
/// reader is a networked one, which campaign missions are too, so a
/// multiplayer role additionally rests on the `MP<n>` name (see the module
/// doc and F56-A).
pub(super) fn classify(
    path: &str,
    members: &BTreeSet<String>,
    has_mis_anim: bool,
    world_groups: &BTreeSet<String>,
) -> Option<(ReaderDirRole, Vec<&'static str>)> {
    let segments: Vec<&str> = path.split(['/', '\\']).collect();
    let has = |name: &str| members.contains(name);
    let per_mission = MISSION_MEMBERS.iter().all(|name| has(name));

    match segments.as_slice() {
        [zbd, group, leaf]
            if zbd.eq_ignore_ascii_case("zbd")
                && world_groups.contains(&group.to_ascii_lowercase())
                && has_mis_anim
                && per_mission =>
        {
            if numbered_leaf(leaf, "ia") && has("ia.zrd") {
                Some((
                    ReaderDirRole::InstantActionScenario,
                    vec!["aiv.zrd", "ia.zrd", "map.zrd", "objectives.zrd"],
                ))
            } else if numbered_leaf(leaf, "mp") && has("net.zrd") {
                Some((
                    ReaderDirRole::MultiplayerScenario,
                    vec!["aiv.zrd", "map.zrd", "net.zrd", "objectives.zrd"],
                ))
            } else {
                None
            }
        }
        // The two installation-scope rules, in the one place they are defined
        // ([`classify_installation_scope`]); here the archive's path shape and
        // its world group's declaration are checked as well.
        [zbd, group]
            if zbd.eq_ignore_ascii_case("zbd")
                && world_groups.contains(&group.to_ascii_lowercase()) =>
        {
            classify_installation_scope(members, has_mis_anim)
        }
        [zbd] if zbd.eq_ignore_ascii_case("zbd") => {
            classify_installation_scope(members, has_mis_anim)
        }
        _ => None,
    }
}

/// Whether `leaf` is `<prefix><digits>` (case-insensitive), e.g. `IA1`.
fn numbered_leaf(leaf: &str, prefix: &str) -> bool {
    let lower = leaf.to_ascii_lowercase();
    lower.strip_prefix(prefix).is_some_and(|digits| {
        !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn members(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    fn groups() -> BTreeSet<String> {
        members(&["c1", "c1c"])
    }

    const MISSION_SHAPE: [&str; 4] = ["aiv.zrd", "map.zrd", "objectives.zrd", "egen.zrd"];

    fn with(extra: &[&str]) -> BTreeSet<String> {
        let mut all: Vec<&str> = MISSION_SHAPE.to_vec();
        all.extend_from_slice(extra);
        members(&all)
    }

    /// The name never decides alone: the corroborating member must be there.
    #[test]
    fn accept_f14_d_1_scenario_roles_need_a_name_and_a_corroborating_member() {
        let ia = classify("ZBD/C1/IA1", &with(&["ia.zrd"]), true, &groups());
        assert_eq!(ia.expect("ia").0, ReaderDirRole::InstantActionScenario);
        let mp = classify("ZBD/C1C/MP3", &with(&["net.zrd"]), true, &groups());
        assert_eq!(mp.expect("mp").0, ReaderDirRole::MultiplayerScenario);

        // An IA-named directory without `ia.zrd`, an MP-named one without
        // `net.zrd`, and a directory named neither stay unclassified.
        assert!(classify("ZBD/C1/IA1", &with(&["net.zrd"]), true, &groups()).is_none());
        assert!(classify("ZBD/C1/MP1", &with(&["ia.zrd"]), true, &groups()).is_none());
        assert!(classify("ZBD/C1/XX1", &with(&["ia.zrd", "net.zrd"]), true, &groups()).is_none());
        assert!(classify("ZBD/C1/IA", &with(&["ia.zrd"]), true, &groups()).is_none());
        // No sibling `mis_anim.zbd`, or an unknown world group: no decision.
        assert!(classify("ZBD/C1/IA1", &with(&["ia.zrd"]), false, &groups()).is_none());
        assert!(classify("ZBD/C9/IA1", &with(&["ia.zrd"]), true, &groups()).is_none());
        // A scenario reader missing a per-mission member is not scenario-shaped.
        assert!(
            classify(
                "ZBD/C1/IA1",
                &members(&["ia.zrd", "map.zrd"]),
                true,
                &groups()
            )
            .is_none()
        );
        // `net.zrd` is no discriminator on its own: every campaign-mission
        // reader lists it too, so a campaign-mission-shaped directory is only
        // ever a scenario when its name says `IA<n>` or `MP<n>`. An `M<nn>`
        // directory the campaign walk does not claim stays unknown instead of
        // joining the denominator as a multiplayer scenario.
        assert!(classify("ZBD/C1C/M05", &with(&["net.zrd"]), true, &groups()).is_none());
    }

    #[test]
    fn accept_f14_d_1_shared_readers_are_classified_and_not_launchable() {
        let world = members(&["templates.zrd", "cam_anim.zrd", "landings.zrd"]);
        let (role, _) = classify("ZBD/C1C", &world, false, &groups()).expect("world group");
        assert_eq!(role, ReaderDirRole::WorldGroupReader);
        assert!(!role.is_launchable());
        // A world-group reader that lists per-mission members is not one.
        assert!(
            classify(
                "ZBD/C1C",
                &with(&["templates.zrd", "cam_anim.zrd"]),
                false,
                &groups()
            )
            .is_none()
        );

        let shared = members(&["instantaction.zrd", "multiplayer_setup.zrd", "ai.zrd"]);
        let (role, _) = classify("ZBD", &shared, false, &groups()).expect("shared");
        assert_eq!(role, ReaderDirRole::SharedReader);
        assert!(!role.is_launchable());
        assert!(classify("ZBD", &members(&["ai.zrd"]), false, &groups()).is_none());
        assert!(ReaderDirRole::InstantActionScenario.is_launchable());
        assert!(ReaderDirRole::MultiplayerScenario.is_launchable());
    }

    /// The scope predicate is the **one** definition of the two installation-scope
    /// rules, and it is decided by the members themselves: a reader that carries
    /// an objective record is never an installation-scope reader, and a
    /// `mis_anim.zbd` beside the archive stops the classification.
    #[test]
    fn accept_f39_e3_an_installation_scope_reader_is_decided_by_its_own_members() {
        let world = members(&[
            "templates.zrd",
            "cam_anim.zrd",
            "landings.zrd",
            "ne000011.zrd",
        ]);
        let (role, evidence) = classify_installation_scope(&world, false).expect("world group");
        assert_eq!(role, ReaderDirRole::WorldGroupReader);
        assert_eq!(evidence, vec!["cam_anim.zrd", "templates.zrd"]);
        assert!(!role.is_launchable());

        let shared = members(&["instantaction.zrd", "multiplayer_setup.zrd", "player.zrd"]);
        let (role, evidence) = classify_installation_scope(&shared, false).expect("shared");
        assert_eq!(role, ReaderDirRole::SharedReader);
        assert_eq!(evidence, vec!["instantaction.zrd", "multiplayer_setup.zrd"]);

        // The measured fact F39-E3 rests on: a reader carrying an objective
        // record is not an installation-scope reader, whatever else it lists.
        assert!(
            classify_installation_scope(&with(&["templates.zrd", "cam_anim.zrd"]), false).is_none()
        );
        assert!(classify_installation_scope(&with(&["targets.zrd"]), false).is_none());
        // A `mis_anim.zbd` beside the archive, a half-matching member set and an
        // empty one all stay undecided instead of guessing a role.
        assert!(classify_installation_scope(&world, true).is_none());
        assert!(classify_installation_scope(&members(&["cam_anim.zrd"]), false).is_none());
        assert!(classify_installation_scope(&members(&["multiplayer_setup.zrd"]), false).is_none());
        assert!(classify_installation_scope(&members(&[]), false).is_none());
    }

    /// The campaign walk and a caller that already holds the archive reach the
    /// same role through the same rule, so the census cannot classify one reader
    /// two ways.
    #[test]
    fn accept_f39_e3_the_scope_predicate_is_the_one_the_campaign_walk_uses() {
        let world = members(&["templates.zrd", "cam_anim.zrd", "landings.zrd"]);
        let shared = members(&["instantaction.zrd", "multiplayer_setup.zrd", "ai.zrd"]);
        for (path, set) in [("ZBD/C1C", &world), ("ZBD", &shared)] {
            let (role, evidence) = classify(path, set, false, &groups()).expect("classified");
            let (direct_role, direct_evidence) =
                classify_installation_scope(set, false).expect("classified directly");
            assert_eq!(role, direct_role);
            assert_eq!(evidence, direct_evidence);
        }
        // A reader the campaign walk refuses on the path or the world-group rule
        // is refused the same way when a caller holds its members: the scope
        // predicate adds no classification of its own.
        assert!(classify("ZBD/C9", &world, false, &groups()).is_none());
        assert!(classify_installation_scope(&world, false).is_some());
    }
}
