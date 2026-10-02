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
//!   reader also lists `net.zrd` is a multiplayer scenario directory. The
//!   name alone never decides: the member must corroborate it;
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
    /// How many members the archive's own index declares.
    pub members: usize,
    /// The member names (lowercase) that corroborate the role, sorted.
    pub evidence: Vec<&'static str>,
}

/// The members a per-mission reader lists and a shared reader does not.
const MISSION_MEMBERS: [&str; 3] = ["map.zrd", "aiv.zrd", "objectives.zrd"];

/// Classifies one directory, or returns `None` when the evidence does not
/// decide.
///
/// `path` is the directory spelling (`ZBD/C1/IA1`), `members` the lowercase
/// member names of its reader archive, `has_mis_anim` whether a
/// `mis_anim.zbd` sits beside the archive, and `world_groups` the lowercase
/// world-group directory names the campaign layout declares.
pub(super) fn classify(
    path: &str,
    members: &BTreeSet<String>,
    has_mis_anim: bool,
    world_groups: &BTreeSet<String>,
) -> Option<(ReaderDirRole, Vec<&'static str>)> {
    let segments: Vec<&str> = path.split(['/', '\\']).collect();
    let has = |name: &str| members.contains(name);
    let per_mission = MISSION_MEMBERS.iter().all(|name| has(name));
    let no_per_mission = MISSION_MEMBERS.iter().all(|name| !has(name));

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
        [zbd, group]
            if zbd.eq_ignore_ascii_case("zbd")
                && world_groups.contains(&group.to_ascii_lowercase())
                && !has_mis_anim
                && no_per_mission
                && has("templates.zrd")
                && has("cam_anim.zrd") =>
        {
            Some((
                ReaderDirRole::WorldGroupReader,
                vec!["cam_anim.zrd", "templates.zrd"],
            ))
        }
        [zbd]
            if zbd.eq_ignore_ascii_case("zbd")
                && !has_mis_anim
                && no_per_mission
                && has("instantaction.zrd")
                && has("multiplayer_setup.zrd") =>
        {
            Some((
                ReaderDirRole::SharedReader,
                vec!["instantaction.zrd", "multiplayer_setup.zrd"],
            ))
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
}
