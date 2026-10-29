//! The mission's taxi/AI path table: the `*_aipath` scene nodes of the
//! world-group `gamez.zbd`, with the point count the original's path-node
//! constructor builds for each (task #2215).
//!
//! The aircraft record's taxi-path field
//! ([`crate::mission_start::TAXI_PATH_FIELD`], `aiv.zrd` field 40) is a
//! *name*; the path itself is scene data: a `<name>_aipath` node whose
//! `<name>_aip0`, `<name>_aip1`, … children are its points. The original's
//! path-node constructor (`0x451440`) spells exactly that with
//! `"%s_aipath"` and `"%s_aip%d"` (`0x624c08`, `0x624c14`) and resolves each
//! name through the by-name scene-node lookup `0x4d0280`. The vehicle
//! loader (`0x47c3a9`) resolves the record's name through the find-or-create
//! path registry `0x4511b0(0x6541b0, 0, name, 1)` and keeps the path only
//! when its point count is at least two (`cmp edx, 2` at `0x47c3f1`): then
//! it stores the path at `V+0xc8` (`0x47c561`), sets `V+0xcc = 1`
//! (`0x47c568`) and places the vehicle at the path's first point. `V+0xcc`
//! is the byte the ground blow's player gate reads to keep a parked
//! aircraft's hit (`0x48c036`..`0x48c0e9`).
//!
//! This resource is the port's half of that registry: which `*_aipath`
//! names the mission's own container carries and how many points each
//! resolves to. [`MissionTaxiPaths::is_on_path`] is the `+0xcc` flag the
//! loader computes; the spawn stores it on
//! [`crate::mission_actors::ScriptedAircraftActor::on_path`]. Path *data*
//! (the point positions, taxi behaviour, the first-point placement) is not
//! imported — only the count the `+0xcc` gate depends on.
//!
//! Read at [`super::compose`] time beside the landing table, through the
//! same `<directory>/gamez.zbd` node array
//! ([`cs_formats::gamez::read_gamez_nodes`]) every world reader of the mount
//! uses. A mount whose container cannot be read keeps an empty table and
//! the reason in `refusals` — never a guessed path.

use std::collections::BTreeMap;
use std::path::Path;

use bevy::prelude::Resource;
use cs_assets::install;
use cs_formats::gamez::{RawNode, read_gamez_nodes};
use cs_formats::io::ParseContext;

/// The suffix the constructor appends to the record's name for the path's
/// scene node: `"%s_aipath"` at `0x624c08`.
const AIPATH_SUFFIX: &str = "_aipath";

/// The point name the constructor's `"%s_aip%d"` (`0x624c14`) spells for
/// `index`.
fn point_name(stem: &str, index: usize) -> String {
    format!("{stem}_aip{index}")
}

/// Resource: every `*_aipath` the mission's world-group container carries,
/// by the name a record's taxi-path field states (`pp1`, `bauda`, …) — the
/// `_aipath` suffix the constructor appends is stripped here, never at the
/// call site.
#[derive(Resource, Clone, Debug, Default)]
pub struct MissionTaxiPaths {
    /// `name` → the count of `<name>_aip<i>` points its `<name>_aipath`
    /// node carries, counting children in the constructor's own `i = 0, 1,
    /// 2, …` order until a name is absent. A path that resolves but carries
    /// fewer than two points still appears — the loader's `cmp edx, 2`
    /// gate is what turns the count into `+0xcc`.
    pub paths: BTreeMap<String, usize>,
    /// Why the container's node array could not be read, when it could not
    /// — a mount with no `gamez.zbd` (a fixture installation) states no
    /// paths and says why here, rather than guessing.
    pub refusals: Vec<String>,
}

impl MissionTaxiPaths {
    /// The resolved point count of `name`'s `<name>_aipath` node — `0`
    /// when the container names no such path. The original's find-or-create
    /// registry then holds an empty path node, which fails the same
    /// two-point gate, so `0` is the honest answer for both "absent" and
    /// "unread".
    #[must_use]
    pub fn point_count(&self, name: &str) -> usize {
        self.paths.get(name).copied().unwrap_or(0)
    }

    /// The vehicle's `+0xcc` byte for a record's taxi-path field: set only
    /// when the named path resolves with at least two points
    /// (`0x47c3f1`). An absent name, an unread container and a short path
    /// all leave it `0`, exactly as the loader's own default does.
    #[must_use]
    pub fn is_on_path(&self, taxi_path: Option<&str>) -> bool {
        taxi_path.is_some_and(|name| self.point_count(name) >= 2)
    }
}

/// Reads `<directory>/gamez.zbd`'s node array into the taxi-path table.
///
/// `directory` is the mount's logical directory (the stage's
/// [`super::compose::StageMount::directory`], e.g. `zbd/c3`) — the same
/// world-group archive the landings and world readers address. The node
/// names are the raw stored names; no coordinate conversion is needed
/// because the table counts points, never positions.
///
/// Row-level refusals are not possible here — either the node array decodes
/// or the whole table stays empty with the reason in
/// [`MissionTaxiPaths::refusals`].
#[must_use]
pub fn read_taxi_paths(install_root: &Path, directory: &str) -> MissionTaxiPaths {
    let mut refusals = Vec::new();
    let paths = read_path_nodes(install_root, directory)
        .map(|nodes| aipath_points(&nodes))
        .unwrap_or_else(|reason| {
            refusals.push(reason);
            BTreeMap::new()
        });
    MissionTaxiPaths { paths, refusals }
}

/// The world-group container's stored node records, or why they could not
/// be read.
fn read_path_nodes(install_root: &Path, directory: &str) -> Result<Vec<RawNode>, String> {
    let found =
        install::discover(install_root).map_err(|error| format!("discovery refuses: {error}"))?;
    let archive = format!("{directory}/gamez.zbd").to_ascii_lowercase();
    let record = found
        .manifest
        .files
        .iter()
        .find(|record| record.relative_spelling.logical_key() == archive)
        .ok_or_else(|| format!("the installation declares no archive {archive}"))?;
    let spelling = record.relative_spelling.as_str().to_owned();
    let bytes = std::fs::read(found.manifest.host_root.join(&spelling))
        .map_err(|error| format!("{archive} could not be read: {error}"))?;
    let mut context = ParseContext::with_defaults(&archive);
    read_gamez_nodes(&mut context, &bytes)
        .map(|nodes| nodes.nodes)
        .map_err(|error| format!("{archive}: {error}"))
}

/// The `name → point count` table of the node array's `*_aipath` records.
///
/// A point is a **child** of the `<name>_aipath` node — every measured
/// `*_aip<i>` record is parented to its path node, and the constructor's
/// `%s_aip%d` loop finds them by name there. The count walks `i = 0, 1, 2,
/// …` and stops at the first absent name, the order the constructor's own
/// lookup loop walks; a gap would end the count exactly as it ends the
/// original's.
fn aipath_points(nodes: &[RawNode]) -> BTreeMap<String, usize> {
    let mut paths = BTreeMap::new();
    for node in nodes {
        let Some(stem) = node.name.strip_suffix(AIPATH_SUFFIX) else {
            continue;
        };
        let children: Vec<&str> = node
            .children
            .iter()
            .filter_map(|slot| nodes.get(*slot as usize))
            .map(|child| child.name.as_str())
            .collect();
        let mut points = 0;
        while children.contains(&point_name(stem, points).as_str()) {
            points += 1;
        }
        paths.insert(stem.to_owned(), points);
    }
    paths
}
