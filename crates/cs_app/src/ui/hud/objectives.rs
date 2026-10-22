//! The current objective line the composed mission draws, and the read model
//! it is drawn from (task #1462 `M01-HUD-OBJECTIVE`, a follow-up of #1438).
//!
//! The F39 read model [`crate::objectives::ObjectiveDisplay`] has **no rows**
//! for an original mission: its rows are seeded from the declared program, and
//! `ObjectiveRecovery::program()` still refuses every original mission, so a
//! mission that runs on the recovered engine model has nothing to show through
//! it. This module takes the other half of that boundary: it projects
//! [`EngineObjectives`] — the measured lifecycle model M01 actually runs — into
//! the identity rows the HUD can show.
//!
//! * **Rows** are the objectives whose `IDENTITY` declaration resolved: role
//!   code, ordinal and the message name the objective runtime never reads but
//!   the briefing/UI table builder consumes (M01-B-OBJ-2).
//! * **`current`** is the live row with the smallest (role code, ordinal,
//!   index) — *live* meaning the model marks it present, not completed and
//!   `Awake`, the state in which the engine evaluates an objective. Which of
//!   several awake objectives the original draws, whether it keeps drawing a
//!   finished one while the record naps its successor, and what it shows when
//!   nothing is awake are **unmeasured** — the measured completion-time HUD
//!   pair carries only a role code and an ordinal — so this choice is
//!   **designed** and the widget labels its layout designed on screen.
//! * **Text** comes from [`MissionMessages`]: the installation's own
//!   `strings.dll`, whose `{name, id}` table pairs a message name with the
//!   `RT_STRING` id its text is stored under. A name that does not resolve is
//!   shown **as the key it is** (spec F12, non-negotiable #4: a missing string
//!   shows a diagnostic key), never as an invented caption, and an identity
//!   with no message element at all draws nothing.
//!
//! Nothing here is `verified_original`: no original run has been observed, and
//! the finding this rests on
//! (`docs/findings/2026-10-15-m01-hud-objective-message-ids.md`) is a reading
//! of the shipped files.

use std::path::Path;

use bevy::prelude::{
    App, BackgroundColor, Color, Commands, Component, Local, Node, Plugin, PositionType, Query,
    Res, Resource, Startup, Text, TextColor, TextFont, UiRect, Update, Val, Visibility, With,
    default,
};
use bevy::text::FontSize;
use cs_content::config::{MessageCatalog, MessageLookup};
use cs_content::objectives_engine::{EngineObjectives, EngineState};
use cs_types::asset_id::SourceSpan;
use cs_types::evidence::ContentHash;

use crate::mission_session::{MissionHost, ObjectiveLifecycles};

/// The file under the installation root that carries both the string rows and
/// the `{name, id}` table the message names resolve through.
pub const MESSAGE_IMAGE: &str = "strings.dll";

/// The caption every line carries, so a reader of the screen can tell this
/// layout apart from an original one (AGENTS.md: a designed layout is
/// labelled designed).
pub const OBJECTIVE_HUD_CAPTION: &str = "OBJECTIVE - DESIGNED LAYOUT";

/// One objective identity the HUD may show: an `IDENTITY` declaration as the
/// engine model resolved it, plus the lifecycle facts this projection reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HudObjective {
    /// The objective's index in the engine model (objective number − 1).
    pub objective: usize,
    /// The loader's role code: `primary` → 1, `secondary` → 2, `tertiary` →
    /// 3, anything else → 0.
    pub role_code: u32,
    /// The `IDENTITY` ordinal, verbatim.
    pub ordinal: u32,
    /// The message name the declaration carried, verbatim and unread by the
    /// objective runtime.
    pub message: Option<String>,
    /// The model marks it present, not completed and `Awake` — the state in
    /// which the engine evaluates it.
    pub live: bool,
    /// The model marks it completed.
    pub completed: bool,
    /// The model marks it present and either completed or no longer
    /// `Dormant` — it has been woken at least once, so the player may know
    /// of it (the pause map's pin flag and note read this).
    pub revealed: bool,
}

impl HudObjective {
    /// The rank this row takes when several are live: role code, then
    /// ordinal, then objective index — so two rows that tie on both still
    /// have one order and the HUD never alternates between them.
    const fn rank(&self) -> (u32, u32, usize) {
        (self.role_code, self.ordinal, self.objective)
    }
}

/// The objective display of the composed mission: every identity row the
/// lifecycle model declares, and which one it currently marks live.
///
/// The model is rebuilt from [`EngineObjectives`] on every read — it holds no
/// state of its own, so it cannot drift from the model it projects, and a
/// consumer that builds it once and keeps it is reading a snapshot it chose
/// to keep.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ObjectiveHud {
    rows: Vec<HudObjective>,
    current: Option<usize>,
}

impl ObjectiveHud {
    /// Projects `engine`: one row per objective whose `IDENTITY` resolved,
    /// in objective order, and the live row with the smallest rank as
    /// [`current`](Self::current).
    ///
    /// A declaration without a role the loader recognised (code 0) still gets
    /// a row — it is an identity the record spells — but it can never be
    /// current, matching the engine's own HUD pair, which is emitted only
    /// for a nonzero role code.
    #[must_use]
    pub fn from_engine(engine: &EngineObjectives) -> Self {
        let mut rows = Vec::new();
        let mut index = 0usize;
        while let Some(objective) = engine.get(index) {
            if let Some(identity) = &objective.declaration.identity {
                let live = identity.role_code != 0
                    && objective.present
                    && !objective.completed
                    && objective.state == EngineState::Awake;
                rows.push(HudObjective {
                    objective: index,
                    role_code: identity.role_code,
                    ordinal: identity.ordinal,
                    message: identity.message.clone(),
                    live,
                    completed: objective.completed,
                    revealed: objective.present
                        && (objective.completed || objective.state != EngineState::Dormant),
                });
            }
            index += 1;
        }
        let current = (0..rows.len())
            .filter(|row| rows[*row].live)
            .min_by_key(|row| rows[*row].rank());
        Self { rows, current }
    }

    /// Every identity row, in objective order.
    #[must_use]
    pub fn rows(&self) -> &[HudObjective] {
        &self.rows
    }

    /// The row the lifecycle model currently marks live, or `None` when none
    /// of them is.
    #[must_use]
    pub fn current(&self) -> Option<&HudObjective> {
        self.current.map(|row| &self.rows[row])
    }

    /// The row the widget draws: [`current`](Self::current) when one is live,
    /// otherwise the pending row — the not-yet-completed identity with the
    /// smallest rank. Every M01 identity block loads dormant and the next one
    /// wakes only after latch hops, so drawing `current` alone left the window
    /// without an objective at launch and between two objectives (owner
    /// playtest 2026-10-17, #1539). Which row the original shows then is
    /// unmeasured, so this fallback is **designed** like the rest of the
    /// layout. `None` once every identity is completed.
    #[must_use]
    pub fn displayed(&self) -> Option<&HudObjective> {
        self.current().or_else(|| {
            self.rows
                .iter()
                .filter(|row| row.role_code != 0 && !row.completed)
                .min_by_key(|row| row.rank())
        })
    }

    /// The text the widget draws for [`displayed`](Self::displayed), resolved
    /// as [`current_text`](Self::current_text) resolves it.
    #[must_use]
    pub fn displayed_text(&self, messages: Option<&MessageCatalog>) -> Option<String> {
        let message = self.displayed()?.message.as_deref()?;
        Some(Self::resolve_text(message, messages))
    }

    /// Whether the model declares no identity at all — the read of a mission
    /// whose record carried no `IDENTITY`, not of one whose identities are
    /// merely not live.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The text the widget draws for [`current`](Self::current).
    ///
    /// * `None` when nothing is current, or when the current identity carries
    ///   no message element — there is no text to look up and inventing one
    ///   would be a caption the record does not have.
    /// * The installation's own text when the message resolves through
    ///   `messages`.
    /// * The message name **as the key** when there is no catalog or the id
    ///   has no row: the diagnostic key spec F12 requires for missing text,
    ///   and the reason a reader of the screen can tell "not loaded" from
    ///   "loaded and empty".
    #[must_use]
    pub fn current_text(&self, messages: Option<&MessageCatalog>) -> Option<String> {
        let message = self.current()?.message.as_deref()?;
        Some(Self::resolve_text(message, messages))
    }

    /// The text of one row, resolved as [`current_text`](Self::current_text)
    /// resolves it; `None` for a row with no message element.
    #[must_use]
    pub fn row_text(row: &HudObjective, messages: Option<&MessageCatalog>) -> Option<String> {
        Some(Self::resolve_text(row.message.as_deref()?, messages))
    }

    fn resolve_text(message: &str, messages: Option<&MessageCatalog>) -> String {
        let Some(catalog) = messages else {
            return message.to_owned();
        };
        match catalog.resolve(message, None) {
            MessageLookup::Found { row, .. } => {
                row.text.clone().unwrap_or_else(|| message.to_owned())
            }
            _ => message.to_owned(),
        }
    }
}

/// The installation's message catalog as the composition read it, or why it
/// could not be read.
///
/// The absence is kept **with its reason** rather than dropped: a HUD that
/// draws message keys instead of text is telling the player-facing surface
/// that the catalog is missing, and a test can ask which of the two happened.
#[derive(Clone, Debug, Resource)]
pub struct MissionMessages(Result<MessageCatalog, String>);

impl MissionMessages {
    /// Reads `MESSAGE_IMAGE` under `install_root`.
    ///
    /// A missing file, a span that will not validate or an image that refuses
    /// becomes the `Err` this resource carries; the read itself never fails,
    /// because "no catalog" is a state the composition must be able to run in
    /// and report, not a reason to abort.
    #[must_use]
    pub fn read(install_root: &Path, install: ContentHash) -> Self {
        let path = install_root.join(MESSAGE_IMAGE);
        let outcome = std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()));
        let outcome = outcome.and_then(|bytes| {
            let length = bytes.len() as u64;
            let span = SourceSpan::new(install, MESSAGE_IMAGE, None, 0, length, None)
                .map_err(|error| format!("{MESSAGE_IMAGE}: {error}"))?;
            let mut context =
                cs_formats::ParseContext::with_defaults(format!("install/{MESSAGE_IMAGE}"));
            MessageCatalog::read(&mut context, span, &bytes)
                .map_err(|error| format!("{MESSAGE_IMAGE}: {error}"))
        });
        Self(outcome)
    }

    /// The catalog, when the installation's own could be read.
    #[must_use]
    pub fn catalog(&self) -> Option<&MessageCatalog> {
        self.0.as_ref().ok()
    }

    /// Why there is no catalog, when there is none.
    #[must_use]
    pub fn missing(&self) -> Option<&str> {
        self.0.as_ref().err().map(String::as_str)
    }
}

impl Default for MissionMessages {
    fn default() -> Self {
        Self(Err(format!(
            "no {MESSAGE_IMAGE} was read for this composition"
        )))
    }
}

/// The widget: one absolutely positioned text node that draws
/// [`MissionMessages`]'s current objective line, windowed compositions only
/// (the headless face has no renderer, and the read model above is what the
/// acceptance tests drive).
#[derive(Clone, Copy, Debug, Default)]
pub struct ObjectiveHudPlugin;

impl Plugin for ObjectiveHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_objective_line)
            .add_systems(Update, update_objective_line);
    }
}

/// The line node the update system finds.
#[derive(Component)]
struct ObjectiveLine;

fn spawn_objective_line(mut commands: Commands) {
    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(18.0),
            ..default()
        },
        TextColor(Color::srgb(1.0, 0.85, 0.2)),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(150.0),
            left: Val::Px(8.0),
            padding: UiRect::all(Val::Px(6.0)),
            ..default()
        },
        Visibility::Hidden,
        ObjectiveLine,
        crate::playtest::visuals::DeveloperOverlayNode,
    ));
}

/// Redraws the line when the model's current objective or its resolved text
/// changes. The mission host is optional: a composition without one draws
/// nothing, and a host on the declared (identity-less) model projects no rows
/// either, so neither case invents a line.
fn update_objective_line(
    host: Option<Res<MissionHost>>,
    messages: Option<Res<MissionMessages>>,
    overlay: Option<Res<crate::playtest::visuals::DeveloperOverlay>>,
    mut lines: Query<(&mut Text, &mut Visibility), With<ObjectiveLine>>,
    mut last: Local<Option<(Option<String>, bool)>>,
) {
    // The caption is a developer label: the original cockpit draws no
    // objective line (its objective is the world-space marker), so the line
    // follows the developer overlay (#1641).
    let overlay_on = crate::playtest::visuals::overlay_visible(overlay.as_deref());
    let line = host.and_then(|host| match host.lifecycles() {
        ObjectiveLifecycles::Engine(engine) => {
            let hud = ObjectiveHud::from_engine(engine);
            hud.displayed_text(messages.as_deref().and_then(MissionMessages::catalog))
        }
        ObjectiveLifecycles::Declared { .. } => None,
    });
    let key = (line, overlay_on);
    if Some(&key) == last.as_ref() {
        return;
    }
    let line = key.0.as_ref();
    let text = line.map_or_else(String::new, |line| {
        format!("{OBJECTIVE_HUD_CAPTION}\n{line}")
    });
    let visible = line.is_some() && overlay_on;
    for (mut node, mut visibility) in &mut lines {
        node.0.clone_from(&text);
        *visibility = if visible {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    *last = Some(key);
}
