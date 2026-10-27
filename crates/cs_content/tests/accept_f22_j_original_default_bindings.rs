//! Acceptance tests for F22-J (#505): the original 2000 PC game's default
//! control bindings, recorded and compared with the project's designed
//! default.
//!
//! Spec: `specs/F22-input-bindings-devices-and-control-ownership.md` (the
//! F22-J follow-up this task files; the task itself is the description).
//! Production code under test: `cs_types::input`'s
//! [`ORIGINAL_DEFAULT_BINDINGS`](cs_types::input::ORIGINAL_DEFAULT_BINDINGS)
//! (the recorded table),
//! [`compare_with_original_default`](cs_types::input::compare_with_original_default)
//! (the comparison against [`ActionMap::designed_default`]) and — with the
//! `retail` capability — the production installation discovery, the
//! `StringCatalog` reader and the production engine-image loader.
//!
//! The table is **static code evidence** recovered from the owner-supplied
//! decrypted image: the owner's analysis (Rally #505 owner note, 2026-10-05)
//! plus this task's bounded scan, which the retail test re-runs against the
//! image bytes. No original executable ran, so nothing here is
//! `verified_original`, and only ids, codes, addresses, spans and digests are
//! asserted — never original bytes or display text.
//!
//! The unignored tests pin the recorded table and the recorded comparison, so
//! CI fails if the production artifact drifts from the measurement; the
//! ignored retail tests need `CS_GAME_DIR` and `CS_ENGINE_IMAGE` and fail
//! loudly (not silently) without them.

use crate::f22_j_support as support;

use crate::f22_h_support as f22_h;

use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};
use cs_content::config::{StringCatalog, StringLookup};
use cs_content::coordinates::{ENGINE_IMAGE_ENV_VAR, ORIGINAL_IMAGE_SHA256, load_engine_image};
use cs_formats::ParseContext;
use cs_types::asset_id::SourceSpan;
use cs_types::input::{
    ActionMap, BindingSource, BindingTarget, DefaultAgreement, DefaultSubject, FlightCommand, Key,
    ORIGINAL_ANALOG_DEFAULTS_SPAN, ORIGINAL_DEFAULT_BINDINGS, ORIGINAL_FLY_MODE_DEFAULT,
    ORIGINAL_FLY_MODE_DEFAULT_VA, ORIGINAL_IMAGE_VA_BIAS, ORIGINAL_JAPANESE_REMAP,
    ORIGINAL_JAPANESE_REMAP_VA, ORIGINAL_JOYSTICK_AXES, ORIGINAL_KEY_ALT, ORIGINAL_KEY_CTRL,
    ORIGINAL_KEY_MAX, ORIGINAL_KEY_SHIFT, ORIGINAL_MOUSE_SENS_DEFAULT,
    ORIGINAL_MOUSE_SENS_DEFAULT_VA, ORIGINAL_REGISTER_VA, ORIGINAL_TABLE_SPAN,
    OriginalControlCategory, OriginalJoystickAxis, UiAction, compare_with_original_default,
    original_binding, original_command_ids, original_command_ids_for_ui, original_scancode,
};
use support::{MEASURED_ROWS, NOT_ON_SCREEN_LABELS, Registration, SCREEN_ONLY_LABELS};

/// The production `StringCatalog` over the installation's `strings.dll`, with
/// the same span construction F22-H's reader uses.
fn read_string_catalog(dir: &std::path::Path) -> (StringCatalog, Vec<u8>) {
    let bytes = f22_h::read_installation_file(dir, "strings.dll");
    let install = fingerprint(&discover(dir).expect("the installation discovers").manifest);
    let span = SourceSpan::new(install, "strings.dll", None, 0, bytes.len() as u64, None)
        .expect("a valid source span");
    let mut context = ParseContext::with_defaults("strings.dll");
    let catalog = StringCatalog::read(&mut context, span, &bytes)
        .expect("the production reader must read strings.dll");
    (catalog, bytes)
}

/// The image's bytes, read through the production engine-image loader, which
/// refuses anything that does not hash to the measured digest.
fn load_image() -> cs_content::coordinates::EngineImage {
    assert!(
        std::env::var_os(ENGINE_IMAGE_ENV_VAR).is_some(),
        "{ENGINE_IMAGE_ENV_VAR} is not set: this test needs the owner-supplied decrypted image"
    );
    load_engine_image().expect("CS_ENGINE_IMAGE names the owner-supplied decrypted image")
}

/// The `len` image bytes starting at virtual address `va`.
fn image_bytes(image: &[u8], va: u32, len: usize) -> &[u8] {
    let offset = (va - ORIGINAL_IMAGE_VA_BIAS) as usize;
    image
        .get(offset..offset + len)
        .unwrap_or_else(|| panic!("the image must cover {len} bytes at VA {va:#x}"))
}

/// The symbol F22-H's measured command labels give label id `id`.
fn measured_label_symbol(id: u32) -> Option<&'static str> {
    f22_h::COMMAND_LABELS
        .iter()
        .find(|(_, label_id)| *label_id == id)
        .map(|(symbol, _)| *symbol)
}

/// **The production artifact is the recorded measurement.** Every row of
/// [`ORIGINAL_DEFAULT_BINDINGS`] equals the recorded row
/// [`MEASURED_ROWS`] holds, field for field, so a transcription error in
/// either fails here — and removing the production table fails to compile.
#[test]
fn accept_f22_j_the_recorded_table_matches_the_production_artifact() {
    assert_eq!(
        ORIGINAL_DEFAULT_BINDINGS.len(),
        64,
        "the original registers 64 commands"
    );
    assert_eq!(MEASURED_ROWS.len(), 64);

    let mut measured = MEASURED_ROWS.iter();
    for row in ORIGINAL_DEFAULT_BINDINGS {
        let (command_id, label_id, category, key_a, key_b, joystick, mouse, va) = measured
            .next()
            .unwrap_or_else(|| panic!("the produced table is longer than the recorded one"));
        assert_eq!(row.command_id, *command_id, "command id of {row:?}");
        assert_eq!(row.label_id, *label_id, "label id of command {command_id}");
        assert_eq!(row.category, *category, "category of command {command_id}");
        assert_eq!(row.key_a.code(), *key_a, "key A of command {command_id}");
        assert_eq!(row.key_b.code(), *key_b, "key B of command {command_id}");
        assert_eq!(
            row.joystick_button, *joystick,
            "joystick button of command {command_id}"
        );
        assert_eq!(
            row.mouse_button, *mouse,
            "mouse button of command {command_id}"
        );
        assert_eq!(
            row.registration_va, *va,
            "registration VA of command {command_id}"
        );
    }
    assert!(
        measured.next().is_none(),
        "the produced table is shorter than the recorded one"
    );

    // Command ids are unique and the rows sit in on-screen order: the category
    // never moves backwards, so the screen's seven groups stay contiguous, and
    // each group holds the recorded number of commands.
    let mut ids: Vec<u16> = ORIGINAL_DEFAULT_BINDINGS
        .iter()
        .map(|row| row.command_id)
        .collect();
    let unique = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), unique, "a command id is registered once");
    for pair in ORIGINAL_DEFAULT_BINDINGS.windows(2) {
        let (first, second) = (pair[0], pair[1]);
        assert!(
            first.category <= second.category,
            "the categories are drawn in order, but {} ({}) follows {} ({})",
            first.category,
            first.command_id,
            second.category,
            second.command_id
        );
    }
    for (category, count) in [
        (OriginalControlCategory::Movement, 7),
        (OriginalControlCategory::Throttle, 11),
        (OriginalControlCategory::Weapons, 6),
        (OriginalControlCategory::Targeting, 11),
        (OriginalControlCategory::View1, 10),
        (OriginalControlCategory::View2, 11),
        (OriginalControlCategory::Other, 8),
    ] {
        assert_eq!(
            ORIGINAL_DEFAULT_BINDINGS
                .iter()
                .filter(|row| row.category == category)
                .count(),
            count,
            "the {category} category holds the recorded number of commands"
        );
    }

    // Every row's ids and codes are inside the ranges the original's encoding
    // allows, so a mistyped field cannot hide in the table.
    for row in ORIGINAL_DEFAULT_BINDINGS {
        assert!(
            row.key_a.code() <= ORIGINAL_KEY_MAX,
            "{row:?}: key A out of range"
        );
        assert!(
            row.key_b.code() <= ORIGINAL_KEY_MAX,
            "{row:?}: key B out of range"
        );
        for key in [row.key_a, row.key_b] {
            assert_eq!(
                key.scancode() == 0,
                key.is_none(),
                "{row:?}: a scancode of 0 is only the unbound code"
            );
            assert!(
                !(key.has_alt() || key.has_ctrl() || key.has_shift()) || !key.is_none(),
                "{row:?}: modifiers require a key"
            );
            assert_eq!(
                key.code() & !(0xff | ORIGINAL_KEY_ALT | ORIGINAL_KEY_CTRL | ORIGINAL_KEY_SHIFT),
                0,
                "{row:?}: the code is a scancode plus the three modifier bits"
            );
        }
        assert!(
            row.joystick_button <= 10,
            "{row:?}: only buttons 1-10 are polled"
        );
        assert_eq!(
            row.mouse_button, 0,
            "{row:?}: no default row binds a mouse button"
        );
    }

    // Every registration call site sits inside the cited table span, and so
    // does the registration function itself.
    for row in ORIGINAL_DEFAULT_BINDINGS {
        assert!(
            row.registration_va >= ORIGINAL_TABLE_SPAN.0
                && row.registration_va < ORIGINAL_TABLE_SPAN.1,
            "{row:?}: the registration call must sit inside the table span"
        );
    }
    assert!(
        ORIGINAL_REGISTER_VA < ORIGINAL_TABLE_SPAN.0
            && ORIGINAL_TABLE_SPAN.0 - ORIGINAL_REGISTER_VA <= 0x1000,
        "the registration function sits just below the span of calls that use it"
    );
    assert!(
        ORIGINAL_ANALOG_DEFAULTS_SPAN.0 >= ORIGINAL_IMAGE_VA_BIAS,
        "a virtual address maps to a file offset through the bias"
    );
}

/// **The table's labels are exactly the measured ones.** Sixty of the 65
/// command labels F22-H measured from `strings.dll` are on the controls
/// screen, the four screen-only labels carry ids outside F22-H's ranges, and
/// the five measured labels the screen does not enumerate are exactly the
/// recorded five — recomputed against F22-H's own list, not asserted by hand.
#[test]
fn accept_f22_j_the_screen_enumerates_the_measured_labels_exactly() {
    let screen_ids: Vec<u32> = ORIGINAL_DEFAULT_BINDINGS
        .iter()
        .map(|row| row.label_id)
        .collect();
    let mut unique_screen = screen_ids.clone();
    unique_screen.sort_unstable();
    unique_screen.dedup();
    assert_eq!(
        unique_screen.len(),
        64,
        "each of the 64 rows names its own label"
    );

    // The screen-only labels are on the screen and outside F22-H's ranges.
    for (symbol, id) in SCREEN_ONLY_LABELS {
        assert!(
            screen_ids.contains(&id),
            "{symbol} ({id}) must be on the controls screen"
        );
        assert!(
            f22_h::command_label_id(symbol).is_none(),
            "{symbol} ({id}) is outside F22-H's measured command ranges"
        );
    }

    // The five measured labels the screen does not enumerate.
    for (symbol, id) in NOT_ON_SCREEN_LABELS {
        assert_eq!(
            f22_h::command_label_id(symbol),
            Some(id),
            "{symbol} must be one of F22-H's measured labels"
        );
        assert!(
            !screen_ids.contains(&id),
            "{symbol} ({id}) must not be on the controls screen"
        );
    }

    // Exactly that split: 60 of the 65 measured labels are on the screen, and
    // nothing else from the measured set is missing.
    let measured_ids: Vec<u32> = f22_h::COMMAND_LABELS.iter().map(|(_, id)| *id).collect();
    let on_screen = measured_ids
        .iter()
        .filter(|id| screen_ids.contains(id))
        .count();
    let excluded: Vec<u32> = measured_ids
        .iter()
        .copied()
        .filter(|id| !screen_ids.contains(id))
        .collect();
    assert_eq!(
        on_screen,
        f22_h::COMMAND_LABELS.len() - NOT_ON_SCREEN_LABELS.len(),
        "the screen enumerates every measured label except the recorded five"
    );
    assert_eq!(
        excluded.len(),
        NOT_ON_SCREEN_LABELS.len(),
        "the excluded labels are exactly the recorded five"
    );
    for (_, id) in NOT_ON_SCREEN_LABELS {
        assert!(
            excluded.contains(&id),
            "label {id} is one of the excluded five"
        );
    }
    assert_eq!(
        on_screen + SCREEN_ONLY_LABELS.len(),
        64,
        "the screen's 64 rows are the 60 measured labels plus the four \
         screen-only ones"
    );
}

/// **The analog defaults and the remap are the recorded ones.** The `FlyMode`
/// and `MouseSens` defaults, the joystick axis mapping and the Japanese-keyboard
/// remap are pinned as values over their cited spans.
#[test]
fn accept_f22_j_the_analog_defaults_and_remap_are_the_recorded_ones() {
    assert_eq!(
        ORIGINAL_FLY_MODE_DEFAULT, 2,
        "the default FlyMode is joystick/key"
    );
    assert_eq!(
        ORIGINAL_MOUSE_SENS_DEFAULT, 1.3,
        "the default MouseSens is 1.3"
    );
    for va in [ORIGINAL_FLY_MODE_DEFAULT_VA, ORIGINAL_MOUSE_SENS_DEFAULT_VA] {
        assert!(
            va >= ORIGINAL_ANALOG_DEFAULTS_SPAN.0 && va < ORIGINAL_ANALOG_DEFAULTS_SPAN.1,
            "the default at {va:#x} must sit inside the analog-defaults span"
        );
    }

    assert_eq!(
        ORIGINAL_JAPANESE_REMAP,
        [(0x0d, 0x90), (0x29, 0x7d), (0x429, 0x47d)],
        "the Japanese-keyboard remap"
    );

    let axes: Vec<(OriginalJoystickAxis, Option<FlightCommand>, bool)> = ORIGINAL_JOYSTICK_AXES
        .iter()
        .map(|binding| (binding.axis, binding.flight_axis, binding.inverted))
        .collect();
    assert_eq!(
        axes,
        vec![
            (OriginalJoystickAxis::X, Some(FlightCommand::Roll), true),
            (OriginalJoystickAxis::Y, Some(FlightCommand::Pitch), false),
            (
                OriginalJoystickAxis::Z,
                Some(FlightCommand::Throttle),
                false
            ),
            (OriginalJoystickAxis::Rz, Some(FlightCommand::Yaw), true),
            (OriginalJoystickAxis::Hat, None, false),
        ],
        "the joystick axis mapping"
    );
}

/// **The recorded chords and the scancode crosswalk are consistent.** Every
/// key code with modifier bits is one of the recorded chords, and the
/// project-authored scancode crosswalk agrees with the original's own rows
/// wherever both name the same physical key.
#[test]
fn accept_f22_j_the_recorded_chords_match_the_scancode_crosswalk() {
    let modifier_bits = ORIGINAL_KEY_ALT | ORIGINAL_KEY_CTRL | ORIGINAL_KEY_SHIFT;
    let mut chords: Vec<u16> = Vec::new();
    for row in ORIGINAL_DEFAULT_BINDINGS {
        for key in [row.key_a, row.key_b] {
            if key.code() & modifier_bits != 0 {
                chords.push(key.code());
            }
        }
    }
    chords.sort_unstable();
    assert_eq!(
        chords,
        vec![
            0x211, 0x212, 0x213, 0x22d, 0x411, 0x412, 0x413, 0x41f, 0x426, 0x429
        ],
        "the ten chord bindings: Shift/Ctrl over E, W, R, L, S, X and backquote"
    );

    // The crosswalk is corroborated by the original's own rows: the rows that
    // bind a key the project's vocabulary also names carry exactly the
    // crosswalk's scancode.
    for (command, key) in [
        (19, Key::Space),
        (69, Key::Escape),
        (9, Key::Digit1),
        (1, Key::ArrowUp),
        (2, Key::ArrowDown),
        (3, Key::ArrowLeft),
        (4, Key::ArrowRight),
        (36, Key::E),
        (39, Key::W),
        (42, Key::R),
        (46, Key::T),
        (56, Key::L),
        (20, Key::X),
        (106, Key::A),
        (35, Key::Tab),
    ] {
        let expected = original_scancode(key).expect("the project key has a scancode");
        assert_eq!(
            original_binding(command)
                .expect("the command is registered")
                .key_a
                .code(),
            expected,
            "command {command} binds the same physical key as the project's {key}"
        );
    }
}

/// **The remap decoder reads the image, not the constant.** The retail test
/// only runs with the image, so this synthetic chain — assembled from the
/// same x86 forms with codes that are **not** the recorded ones — pins the
/// decoder itself in CI: it recovers the pairs the chain encodes, and an
/// image holding no chain decodes to nothing rather than to the recorded
/// table.
#[test]
fn accept_f22_j_the_remap_decoder_reads_what_it_is_given() {
    let synthetic = [(0x0011, 0x0021), (0x002f, 0x0044), (0x0145, 0x028a)];
    let code = support::synthetic_remap(synthetic);
    let start = (ORIGINAL_JAPANESE_REMAP_VA - ORIGINAL_IMAGE_VA_BIAS) as usize;
    let mut image = vec![0u8; start + code.len()];
    image[start..].copy_from_slice(&code);

    assert_eq!(
        support::decode_japanese_remap(&image),
        Some(synthetic),
        "the decoder recovers exactly the pairs the chain encodes"
    );
    assert_eq!(
        support::decode_japanese_remap(&vec![0u8; start + code.len()]),
        None,
        "an image that holds no chain decodes to nothing, not to the table"
    );
}

/// The recorded comparison of the designed default against the original
/// default: one entry per declared command and UI action, in the order
/// [`compare_with_original_default`] emits them.
const RECORDED_COMPARISON: &[(&str, &str)] = &[
    ("pitch", "different"),
    ("roll", "different"),
    ("yaw", "different"),
    ("throttle", "different"),
    ("fire_primary", "partial"),
    ("fire_secondary", "different"),
    ("cycle_weapon", "different"),
    ("drop_ordnance", "project_only"),
    ("eject", "different"),
    ("toggle_gear", "project_only"),
    ("flap_step", "project_only"),
    ("target_next", "different"),
    ("target_prev", "different"),
    // PARITY2-M01-TARGET-INDICATORS (#1727): the eleven measured selection
    // commands cite real original rows (command ids 36–46) but the designed
    // default binds them nothing — a `Shift`/`Ctrl` chord is not a
    // `BindingSource`; the mission mints their edges from raw key state
    // (`mint_mission_target_keys`). A declared subject with original rows
    // and no designed source records `original_only`.
    ("target_next_enemy", "original_only"),
    ("target_prev_enemy", "original_only"),
    ("target_nearest_enemy", "original_only"),
    ("target_next_ally", "original_only"),
    ("target_prev_ally", "original_only"),
    ("target_nearest_ally", "original_only"),
    ("target_next_ground", "original_only"),
    ("target_prev_ground", "original_only"),
    ("target_nearest_ground", "original_only"),
    ("target_under_reticule", "original_only"),
    ("target_nothing", "original_only"),
    ("countermeasure", "project_only"),
    ("throttle_step_up", "different"),
    ("throttle_step_down", "different"),
    ("throttle_idle", "same"),
    ("throttle_full", "different"),
    // PARITY2-ORIGINAL-KEYBINDINGS (#1733): the seven direct throttle levels
    // cite original rows (command ids 10-16) the developer map binds nothing.
    ("throttle_level_1", "original_only"),
    ("throttle_level_2", "original_only"),
    ("throttle_level_3", "original_only"),
    ("throttle_level_4", "original_only"),
    ("throttle_level_5", "original_only"),
    ("throttle_level_6", "original_only"),
    ("throttle_level_7", "original_only"),
    ("level_off", "different"),
    ("confirm", "runtime_only"),
    ("cancel", "runtime_only"),
    ("navigate_up", "runtime_only"),
    ("navigate_down", "runtime_only"),
    ("navigate_left", "runtime_only"),
    ("navigate_right", "runtime_only"),
    ("pause", "same"),
];

/// **The comparison is recorded and covers both vocabularies exactly.**
/// [`compare_with_original_default`] classifies every declared command and UI
/// action once, every original command once, and the classifications are the
/// recorded ones — including the two defaults that agree (`throttle_idle`,
/// `pause`) and the one partial overlap (`fire_primary`, which shares the
/// space bar with the original's Fire Guns).
#[test]
fn accept_f22_j_the_designed_default_comparison_is_recorded() {
    let map = ActionMap::designed_default();
    assert_eq!(
        map.bindings().len(),
        // 42 bindings plus F22-G's flight-context pause request.
        43,
        "the comparison must run against the real designed default map"
    );
    let rows = compare_with_original_default(&map);

    // One row per declared command, one per declared UI action and one per
    // original-only command, with nothing counted twice.
    let declared: Vec<&str> = rows
        .iter()
        .filter_map(|row| match row.subject {
            DefaultSubject::Declared(label) => Some(label),
            DefaultSubject::OriginalOnly(_) => None,
        })
        .collect();
    let original_only: Vec<u16> = rows
        .iter()
        .filter_map(|row| match row.subject {
            DefaultSubject::Declared(_) => None,
            DefaultSubject::OriginalOnly(id) => Some(id),
        })
        .collect();
    assert_eq!(
        declared.len(),
        FlightCommand::ALL.len() + UiAction::ALL.len(),
        "every declared command and UI action is compared exactly once"
    );
    assert_eq!(
        original_only.len() + 37,
        ORIGINAL_DEFAULT_BINDINGS.len(),
        "every original command is either compared with a declared command or \
         recorded as original-only"
    );

    let mut recorded = RECORDED_COMPARISON.iter();
    for label in &declared {
        let (expected_label, expected) = recorded
            .next()
            .unwrap_or_else(|| panic!("{label} is not in the recorded comparison"));
        let row = rows
            .iter()
            .find(|row| matches!(row.subject, DefaultSubject::Declared(name) if name == *label))
            .unwrap_or_else(|| panic!("{label} has no comparison row"));
        assert_eq!(label, expected_label, "the recorded comparison's order");
        let expected = match *expected {
            "same" => DefaultAgreement::Same,
            "partial" => DefaultAgreement::Partial,
            "different" => DefaultAgreement::Different,
            "project_only" => DefaultAgreement::ProjectOnly,
            "original_only" => DefaultAgreement::OriginalOnly,
            "runtime_only" => DefaultAgreement::RuntimeOnly,
            other => panic!("the recorded comparison names an unknown status {other}"),
        };
        assert_eq!(
            row.agreement, expected,
            "the agreement recorded for {label}"
        );
    }
    assert!(
        recorded.next().is_none(),
        "the recorded comparison is longer than the declared vocabulary"
    );

    // The two defaults that agree, spelled out, and the one partial overlap.
    let row = |label: &str| {
        rows.iter()
            .find(|row| matches!(row.subject, DefaultSubject::Declared(name) if name == label))
            .unwrap_or_else(|| panic!("{label} has no comparison row"))
    };
    let pause = row("pause");
    assert_eq!(
        (pause.designed.as_slice(), pause.original.as_slice()),
        (
            ["key.escape".to_owned()].as_slice(),
            ["cmd 69 label 11037 key_a 0x001 key_b - joy 0 mouse 0".to_owned()].as_slice(),
        ),
        "both defaults bind Escape to pause"
    );
    let idle = row("throttle_idle");
    assert_eq!(
        (idle.designed.as_slice(), idle.original.as_slice()),
        (
            ["key.digit1".to_owned()].as_slice(),
            ["cmd 9 label 11069 key_a 0x002 key_b - joy 0 mouse 0".to_owned()].as_slice(),
        ),
        "both defaults bind the 1 key to throttle idle"
    );
    let fire = row("fire_primary");
    assert_eq!(fire.agreement, DefaultAgreement::Partial);
    assert_eq!(
        fire.designed,
        vec![
            "key.space".to_owned(),
            "mouse_button.left".to_owned(),
            "gamepad_button.south".to_owned(),
            "joystick_button.0".to_owned(),
        ],
        "the designed default binds four sources to fire_primary"
    );
    assert_eq!(
        fire.original,
        vec!["cmd 19 label 11045 key_a 0x039 key_b - joy 1 mouse 0".to_owned()],
        "the original binds the space bar and joystick button 1"
    );
    let level = row("level_off");
    assert_eq!(
        (level.designed.as_slice(), level.original.as_slice()),
        (
            ["key.l".to_owned()].as_slice(),
            ["cmd 47 label 11061 key_a 0x426 key_b - joy 0 mouse 0".to_owned()].as_slice(),
        ),
        "the original's Level-Off assist is the chord Shift+L, the designed \
         default the bare letter"
    );

    // The original-only set is exactly the commands no declared command cites:
    // the crosswalk's complement — the padlock/look/chat rows and the other
    // commands the project declares nothing for.
    let mut cited: Vec<u16> = FlightCommand::ALL
        .iter()
        .flat_map(|command| original_command_ids(*command).iter().copied())
        .chain(
            UiAction::ALL
                .iter()
                .flat_map(|action| original_command_ids_for_ui(*action).iter().copied()),
        )
        .collect();
    cited.sort_unstable();
    cited.dedup();
    let mut expected_original_only: Vec<u16> = ORIGINAL_DEFAULT_BINDINGS
        .iter()
        .map(|row| row.command_id)
        .filter(|id| !cited.contains(id))
        .collect();
    expected_original_only.sort_unstable();
    let mut sorted_rows = original_only.clone();
    sorted_rows.sort_unstable();
    assert_eq!(
        sorted_rows, expected_original_only,
        "the produced original-only set is the crosswalk's complement"
    );
    assert_eq!(
        sorted_rows,
        vec![
            18, 35, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67,
            68, 106, 109, 110, 111
        ],
        "the 27 original-only commands, by id"
    );

    // The project default stays designed: the comparison changed nothing, and
    // the designed Level-Off slot is still the bare letter rather than the
    // original's chord.
    assert_eq!(
        map.target_for(BindingSource::Key(Key::L)),
        Some(BindingTarget::Command(FlightCommand::LevelOff)),
        "the designed default keeps its own Level-Off binding"
    );
    assert_eq!(map, ActionMap::designed_default());
}

/// **Retail: the image registers exactly the recorded table.** The production
/// discovery fingerprints the installation, the production engine-image loader
/// reads the decrypted image (refusing anything that does not hash to the
/// measured digest), and this task's bounded scan decodes every registration
/// call in the table span and matches it, field for field, against the
/// production table.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_f22_j_retail_the_image_registers_the_recorded_defaults() {
    let dir = f22_h::game_dir();
    let found = discover(&dir).expect("the installation discovers");
    assert_eq!(
        fingerprint(&found.manifest).to_hex(),
        f22_h::INSTALL_SHA256,
        "the installation fingerprint must match the measured one"
    );
    assert_eq!(
        content_fingerprint(&found.manifest).to_hex(),
        f22_h::CONTENT_SHA256,
        "the canonical-content fingerprint must match the measured one"
    );

    let image = load_image();
    assert_eq!(
        image.digest.to_hex(),
        ORIGINAL_IMAGE_SHA256,
        "the decrypted image is the measured one"
    );

    let decoded = support::decode_registrations(&image.bytes);
    assert_eq!(
        decoded.len(),
        64,
        "the table span holds exactly 64 registration calls"
    );
    let mut decoded = decoded.into_iter();
    for row in ORIGINAL_DEFAULT_BINDINGS {
        let Registration {
            command_id,
            label_id,
            key_a,
            key_b,
            joystick_button,
            mouse_button,
            call_va,
        } = decoded
            .next()
            .unwrap_or_else(|| panic!("the image registers fewer commands than the table holds"));
        assert_eq!(command_id, row.command_id, "the command id at {call_va:#x}");
        assert_eq!(label_id, row.label_id, "command {command_id}'s label id");
        assert_eq!(key_a, row.key_a.code(), "command {command_id}'s key A");
        assert_eq!(key_b, row.key_b.code(), "command {command_id}'s key B");
        assert_eq!(
            joystick_button, row.joystick_button,
            "command {command_id}'s joystick button"
        );
        assert_eq!(
            mouse_button, row.mouse_button,
            "command {command_id}'s mouse button"
        );
        assert_eq!(
            call_va, row.registration_va,
            "command {command_id}'s registration call site"
        );
    }
    assert!(
        decoded.next().is_none(),
        "the image registers more commands than the table holds"
    );

    // The Japanese-keyboard remap: the three code pairs its function at the
    // recorded address encodes are exactly the recorded ones. The scanner
    // reads them out of the image, so what is asserted here is the recorded
    // code pairs — never a reproduced copy of the function's body.
    assert_eq!(
        support::decode_japanese_remap(&image.bytes),
        Some(ORIGINAL_JAPANESE_REMAP),
        "the remap at {ORIGINAL_JAPANESE_REMAP_VA:#x} must encode the recorded code pairs"
    );

    // The two analog defaults: a `push 2` (FlyMode) and a `push 1.3f`
    // (MouseSens) at their recorded addresses inside the cited span.
    assert_eq!(
        image_bytes(&image.bytes, ORIGINAL_FLY_MODE_DEFAULT_VA, 2),
        [0x6a, ORIGINAL_FLY_MODE_DEFAULT],
        "the FlyMode default is pushed at its recorded address"
    );
    let mut expected_mouse_sens = vec![0x68];
    expected_mouse_sens.extend_from_slice(&support::f32_bits(ORIGINAL_MOUSE_SENS_DEFAULT));
    assert_eq!(
        image_bytes(&image.bytes, ORIGINAL_MOUSE_SENS_DEFAULT_VA, 5),
        expected_mouse_sens.as_slice(),
        "the MouseSens default is pushed at its recorded address"
    );

    // Every cited span lies inside the image's mapped range.
    let extent = image.bytes.len() as u64;
    for (name, span) in [
        ("table", ORIGINAL_TABLE_SPAN),
        ("analog defaults", ORIGINAL_ANALOG_DEFAULTS_SPAN),
    ] {
        assert!(
            u64::from(span.0 - ORIGINAL_IMAGE_VA_BIAS) < extent
                && u64::from(span.1 - ORIGINAL_IMAGE_VA_BIAS) <= extent,
            "the {name} span must lie inside the image"
        );
    }
}

/// **Retail: `strings.dll` names every recorded label.** The production
/// `StringCatalog` resolves each row's label id, and the `.data`
/// `{name, id}` table gives each recorded symbol its recorded id — including
/// the four screen-only labels outside F22-H's measured ranges.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f22_j_retail_strings_dll_names_every_recorded_label() {
    let dir = f22_h::game_dir();
    let (catalog, bytes) = read_string_catalog(&dir);
    assert_eq!(
        sha256(&bytes).to_hex(),
        f22_h::STRINGS_DLL_SHA256,
        "strings.dll must be the measured image"
    );
    let table = f22_h::parse_name_id_table(&bytes);
    assert_eq!(table.len(), f22_h::NAME_ID_ENTRIES);

    let resolve = |id: u32| match catalog.resolve(id, Some(f22_h::STRINGS_LANGUAGE)) {
        StringLookup::Found(row) => assert!(
            !row.code_units.is_empty(),
            "label {id} must not decode to an empty label"
        ),
        other => panic!("label {id} must resolve through StringCatalog, got {other:?}"),
    };

    // Every row's label id resolves and its symbol is named with that id.
    for row in ORIGINAL_DEFAULT_BINDINGS {
        let symbol = measured_label_symbol(row.label_id)
            .or_else(|| {
                SCREEN_ONLY_LABELS
                    .iter()
                    .find(|(_, id)| *id == row.label_id)
                    .map(|(symbol, _)| *symbol)
            })
            .unwrap_or_else(|| {
                panic!(
                    "command {}'s label {} has no recorded symbol",
                    row.command_id, row.label_id
                )
            });
        assert_eq!(
            table.get(symbol).copied(),
            Some(row.label_id),
            "{symbol} must name id {} in the .data table",
            row.label_id
        );
        resolve(row.label_id);
    }

    // The seven category headers resolve and are named, so the screen's
    // grouping is measured rather than assumed.
    for (symbol, id) in f22_h::CONTROL_CATEGORIES {
        assert_eq!(
            table.get(symbol).copied(),
            Some(id),
            "{symbol} names its header id"
        );
        resolve(id);
    }

    // The four screen-only labels are named and sit outside F22-H's two
    // measured command ranges, which is why F22-H did not measure them.
    for (symbol, id) in SCREEN_ONLY_LABELS {
        assert_eq!(
            table.get(symbol).copied(),
            Some(id),
            "{symbol} must name id {id}"
        );
        assert!(
            !(10_000..=10_030).contains(&id) && !(11_000..=11_100).contains(&id),
            "{symbol} ({id}) is outside F22-H's measured command ranges"
        );
        resolve(id);
    }

    // The five measured labels the screen does not enumerate still resolve:
    // they are measured vocabulary, just not default-screen rows.
    for (symbol, id) in NOT_ON_SCREEN_LABELS {
        assert_eq!(f22_h::command_label_id(symbol), Some(id));
        assert!(
            !ORIGINAL_DEFAULT_BINDINGS
                .iter()
                .any(|row| row.label_id == id),
            "{symbol} must not be a default-screen row"
        );
        resolve(id);
    }
}
