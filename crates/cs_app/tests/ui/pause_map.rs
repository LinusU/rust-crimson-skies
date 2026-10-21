//! Task PARITY3-PAUSE-MAP-SCREEN.01 (#1816): the retail probe behind
//! `docs/findings/2026-10-21-pause-map-screen-data.md`.
//!
//! The original Esc screen (`MSG_CMD_PAUSE_GAME`) is described by
//! `escape.zrd` in `ZBD/zrdr.zbd`; its bitmaps are textures of `ZBD/rimage.zbd`.
//! These tests read the located members through the production readers (the
//! UI texture catalog behind [`FrontEndScreens`], the version-one ZBD index and
//! the reader archive, [`decode_zrd`]) and assert the extents, digests and
//! values the note records. They read the installation only and keep nothing of
//! it. Every `retail` test fails loudly without `CS_GAME_DIR`.

use std::path::PathBuf;

use cs_app::ui::front_end::{FrontEndScreens, ImageSource, OriginalImage};
use cs_assets::install::sha256;
use cs_content::stunts::{ZrdValue, decode_zrd, zrd_field};
use cs_formats::io::ParseContext;
use cs_formats::zbd::{ZbdProbe, dispatch, read_reader_archive, read_version_one_index};

/// The mission-independent reader archive that holds the Esc, loading and
/// briefing scripts.
const COMMON_READER: &str = "ZBD/zrdr.zbd";
/// M01's own reader archive (`zbd/c3/m01`).
const M01_READER: &str = "ZBD/C3/M01/zrdr.zbd";
/// The Esc screen's script key for M01 (chapter 1 mission 1, `brief_c61`).
const M01_KEY: &str = "loading_c61";

fn game_dir() -> PathBuf {
    let dir = PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must point at the original installation"),
    );
    assert!(
        dir.is_dir(),
        "CS_GAME_DIR {} is not a directory",
        dir.display()
    );
    dir
}

/// One member of a reader archive: its stored bytes, read through production.
fn reader_member(container: &str, member: &str) -> Vec<u8> {
    let bytes = std::fs::read(game_dir().join(container)).expect("the container reads");
    let spelling = cs_types::install::RelativePath::new(container).expect("a relative path");
    let mut context = ParseContext::with_defaults("pause_map_probe");
    let decision = dispatch(ZbdProbe::new(container, &spelling, &bytes)).expect("a ZBD");
    let index =
        read_version_one_index(&mut context, decision, &bytes).expect("the version-one index");
    let table = index.member_table();
    let archive =
        read_reader_archive(&mut context, &table, index.data()).expect("the reader archive");
    archive
        .entries()
        .find(|entry| String::from_utf8_lossy(entry.name()).eq_ignore_ascii_case(member))
        .map(|entry| entry.content().to_vec())
        .unwrap_or_else(|| panic!("{container} has no member {member}"))
}

fn document(container: &str, member: &str) -> ZrdValue {
    let tree = decode_zrd(&reader_member(container, member)).expect("the member decodes");
    // The reader wraps the document in one more list.
    tree.as_list()
        .and_then(|children| children.first())
        .cloned()
        .expect("a document node")
}

fn screens() -> &'static FrontEndScreens {
    static SCREENS: std::sync::OnceLock<FrontEndScreens> = std::sync::OnceLock::new();
    SCREENS.get_or_init(|| FrontEndScreens::open(&game_dir()).expect("the screen artwork opens"))
}

fn inventory() -> &'static Vec<OriginalImage> {
    static IMAGES: std::sync::OnceLock<Vec<OriginalImage>> = std::sync::OnceLock::new();
    IMAGES.get_or_init(|| screens().inventory().expect("the inventory reads").images)
}

fn field<'a>(node: &'a ZrdValue, key: &str) -> &'a ZrdValue {
    zrd_field(node, key).unwrap_or_else(|| panic!("no field {key}"))
}

/// The value after the text `key` among a command's flat arguments
/// (`["OBJPIN1" "bitmap" ["pin6"] "at" [506 149]]`), which [`zrd_field`]'s
/// name/value pairing does not follow once an argument is a bare name.
fn argument<'a>(command: &'a ZrdValue, key: &str) -> &'a ZrdValue {
    let children = command.as_list().expect("arguments");
    let at = children
        .iter()
        .position(|child| child.as_text() == Some(key))
        .unwrap_or_else(|| panic!("no argument {key}"));
    &children[at + 1]
}

fn ints(node: &ZrdValue) -> Vec<u32> {
    node.as_list()
        .expect("a list")
        .iter()
        .map(|value| match value {
            ZrdValue::Int(int) => *int,
            other => panic!("not an int: {other:?}"),
        })
        .collect()
}

fn floats(node: &ZrdValue) -> Vec<f32> {
    node.as_list()
        .expect("a list")
        .iter()
        .map(|value| match value {
            ZrdValue::Float(float) => *float,
            ZrdValue::Int(int) => *int as f32,
            other => panic!("not a number: {other:?}"),
        })
        .collect()
}

fn text(node: &ZrdValue) -> &str {
    node.as_list()
        .and_then(|children| children.first())
        .and_then(ZrdValue::as_text)
        .expect("a one-text list")
}

/// Every text value anywhere under `node`.
fn texts<'a>(node: &'a ZrdValue, out: &mut Vec<&'a str>) {
    match node {
        ZrdValue::Text(value) => out.push(value),
        ZrdValue::List(children) => children.iter().for_each(|child| texts(child, out)),
        ZrdValue::Int(_) | ZrdValue::Float(_) => {}
    }
}

fn texture(name: &str) -> &'static OriginalImage {
    inventory()
        .iter()
        .find(|image| image.source == ImageSource::UiArchive && image.name == name)
        .unwrap_or_else(|| panic!("rimage.zbd lists no texture {name}"))
}

fn container_image(spelling: &str) -> &'static OriginalImage {
    inventory()
        .iter()
        .find(|image| image.source == ImageSource::ScreenContainer && image.name == spelling)
        .unwrap_or_else(|| panic!("crimson.rof lists no member {spelling}"))
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity3_pause_map_01_scripts_are_the_recorded_members() {
    // (container, member, stored length, SHA-256 of the stored bytes)
    let expected = [
        (
            COMMON_READER,
            "escape.zrd",
            220_066,
            "c1c9694ddecb96ffaa1cd9b08fed067b6c808e744f720f39f7f4f5bdc0edb74d",
        ),
        (
            COMMON_READER,
            "ia_escape.zrd",
            222_536,
            "6237bd2a3811491a5a7de7a2b875952476676f014917682bbe48c254e6e732f6",
        ),
        (
            COMMON_READER,
            "Loading.zrd",
            252_737,
            "c610d865dcffe16e4bfff88b7743aafeaf29c2e11df47b96d4c1c60ea6f360b9",
        ),
        (
            COMMON_READER,
            "map_anims.zrd",
            1_630,
            "c05d263a160e53e244dabfa0b2d7187b5d6625b2d2b09bfe288a61cacc521123",
        ),
        (
            COMMON_READER,
            "fonts.zrd",
            13_902,
            "a4ad1d84e02e0521cff262c611d936fef94aabf3a610f359ac69e6a76cebfed6",
        ),
        (
            M01_READER,
            "map.zrd",
            651,
            "07539cae3ae0409e5af554e6d257ddefb3beec39e33b5e4dfd9a82f42c254ce7",
        ),
        (
            M01_READER,
            "objectives.zrd",
            10_663,
            "f632a5d88c7781f86418e1380debb4d9c0b8fa17e68e27061bd31a8b47c3fca1",
        ),
        (
            M01_READER,
            "location.zrd",
            286,
            "ac0f9c344a2bd8aa5481284e696109466c9d7a7cf53ce5604473a1ab90930406",
        ),
    ];
    for (container, member, length, digest) in expected {
        let bytes = reader_member(container, member);
        assert_eq!(bytes.len(), length, "{container}#{member} length");
        assert_eq!(
            sha256(&bytes).to_hex(),
            digest,
            "{container}#{member} digest"
        );
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity3_pause_map_01_m01_escape_entry_states_the_chart_pins_and_photo() {
    let doc = document(COMMON_READER, "escape.zrd");
    let entry = field(&doc, M01_KEY);

    // (a) the chart: the M01 briefing map, drawn at (16,19), clipped, with a
    // WORLD rectangle.
    let primitives = field(entry, "PRIMITIVES");
    let map = field(primitives, "MAP");
    assert_eq!(text(field(map, "BITMAP")), "HA-m1MAP");
    assert_eq!(ints(field(map, "POSITION")), [16, 19]);
    let clip = field(map, "CLIP");
    assert_eq!(ints(field(clip, "topleft")), [211, 51]);
    assert_eq!(ints(field(clip, "bottomright")), [784, 551]);
    let world = field(map, "WORLD");
    assert_eq!(floats(field(world, "topleft")), [-10818.43, 9155.875]);
    assert_eq!(floats(field(world, "bottomright")), [-2204.118, 540.563]);

    // (e) the photo: `momento_temp` at (533,326), shadow `momento_shad`.
    let memento = field(primitives, "MEMENTO");
    assert_eq!(text(field(memento, "BITMAP")), "momento_temp");
    assert_eq!(ints(field(memento, "POSITION")), [533, 326]);

    // (b) the pins: authored screen positions and bitmaps, no world position.
    let script = field(entry, "ESC_SCRIPT").as_list().expect("a script");
    let mut pins = Vec::new();
    let mut other = Vec::new();
    for pair in script.chunks(2) {
        if pair[0].as_text() == Some("Pict") {
            let args = pair[1].as_list().expect("arguments");
            let name = args[0].as_text().expect("a name");
            let bitmap = text(argument(&pair[1], "bitmap")).to_owned();
            let at = ints(argument(&pair[1], "at"));
            if name.starts_with("OBJPIN") {
                pins.push((name.to_owned(), bitmap, at));
            } else {
                other.push((name.to_owned(), bitmap, at));
            }
        }
    }
    let pin =
        |name: &str, bitmap: &str, x: u32, y: u32| (name.to_owned(), bitmap.to_owned(), vec![x, y]);
    assert_eq!(
        pins,
        [
            pin("OBJPIN1", "pin6", 506, 149),
            pin("OBJPIN2", "pin6", 396, 254),
            pin("OBJPIN3", "pin6", 181, 244),
            pin("OBJPIN4", "pin4", 106, 319),
        ]
    );
    assert_eq!(other, [pin("MOMENTOSHAD", "momento_shad", 661, 454)]);
    assert_eq!(
        (
            texture("pin6").width,
            texture("pin6").height,
            texture("pin4").width
        ),
        (76, 80, 76)
    );
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity3_pause_map_01_dialog_names_player_icon_note_buttons_and_fonts() {
    let doc = document(COMMON_READER, "escape.zrd");
    let dialog = field(&doc, "LOADINGDIALOG");

    // (c) the player aircraft icon and (d) the objectives note.
    let primitives = field(dialog, "PRIMITIVES");
    assert_eq!(
        text(field(field(primitives, "OWNSHIP"), "BITMAP")),
        "singledev"
    );
    let list = field(primitives, "OBJECTIVESLIST");
    assert_eq!(
        text(field(field(list, "BACKGROUND"), "BITMAP")),
        "parchment"
    );
    assert_eq!(ints(field(field(list, "BACKGROUND"), "POSITION")), [555, 6]);
    assert_eq!(
        text(field(field(list, "TITLE"), "TEXT")),
        "MSG_BRF_DLG_OBJECTIVES"
    );
    assert_eq!(text(field(field(list, "TITLE"), "FONT")), "ObjListTitle");
    assert_eq!(text(field(field(list, "LIST"), "FONT")), "ObjList");
    assert_eq!(ints(field(field(list, "LIST"), "POSITION")), [580, 50]);
    assert_eq!(ints(field(field(list, "LIST"), "WORDWRAP")), [190, 255]);
    assert_eq!(
        text(field(field(list, "CHECKMARK"), "BITMAP")),
        "obj_check1"
    );

    // (f) the four buttons: positions, labels and the three state bitmaps.
    let buttons = field(dialog, "BUTTONS");
    for (key, label, x, y) in [
        ("RESUME_MISSION_BTN", "MSG_BTN_RESUME", 107, 528),
        ("RESTART_MISSION_BTN", "MSG_BTN_RESTART", 127, 559),
        ("CONFIGURE_BTN", "MSG_BTN_PREFERENCES", 367, 528),
        ("MAINMENU_BTN", "MSG_BTN_QUIT", 347, 559),
    ] {
        let button = field(buttons, key);
        assert_eq!(ints(field(button, "POSITION")), [x, y], "{key}");
        assert_eq!(text(field(button, "BITMAP")), "escape_button1", "{key}");
        let normal = field(button, "LABEL").as_list().expect("a label");
        assert_eq!(normal[0].as_text(), Some(label), "{key}");
        assert_eq!(
            text(field(field(button, "ROLLOVER"), "BITMAP")),
            "escape_button2"
        );
        assert_eq!(
            text(field(field(button, "ACTIVATE"), "BITMAP")),
            "escape_button3"
        );
    }

    // The cursor of every entry.
    let cursor = field(field(&doc, "default"), "CURSOR");
    assert_eq!(text(field(cursor, "BITMAP")), "daglove");
    assert_eq!(text(field(field(cursor, "ROLLOVER"), "BITMAP")), "dafinger");

    // The note's fonts are Windows faces named by `fonts.zrd`.
    let fonts = document(COMMON_READER, "fonts.zrd");
    let windows = field(&fonts, "WINDOWS_FONTS");
    assert_eq!(text(field(field(windows, "ObjList"), "face")), "Andy Bold");
    assert_eq!(
        text(field(field(windows, "ObjListTitle"), "face")),
        "Andy Bold"
    );
    assert_eq!(
        text(field(field(windows, "BtnEscapeNormal"), "face")),
        "Trebuchet MS Bold"
    );
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity3_pause_map_01_art_extents_and_digests() {
    // (name, width, height, SHA-256 of the decoded texel plane)
    let textures = [
        (
            "ha-m1map",
            800,
            600,
            "557cd19cf8aaba3acc11451b11bd011f7fd3239bf365ba090d23cfaacc449992",
        ),
        (
            "pin1",
            76,
            80,
            "40ced0abdafe6f2971940c955e56d0f52758398dc5a5d5aeec55f0282274d8a4",
        ),
        (
            "pin4",
            76,
            80,
            "6941145b860443d1294b50eaa3976c00dd53069bbafa2ffb596468a3f4870ba3",
        ),
        (
            "pin6",
            76,
            80,
            "28ddc10069ad4e7b9340fd2cbae6f314710a15feaca315c5f61f87c05c6eebf5",
        ),
        (
            "pin6_1",
            76,
            80,
            "5b67169c5219928211b6e5be08f70eef022629c56c44d8f9e24141fc07c078f6",
        ),
        (
            "singledev",
            32,
            32,
            "66ce106ccdeb61f791b0a0012ef4dfffc775c246131b6c02fdb5494fd5166df2",
        ),
        (
            "parchment",
            240,
            312,
            "c29384c58046291c189061098094459e278d6c6b1076a511b5f8047b5912e0ba",
        ),
        (
            "obj_check1",
            25,
            24,
            "7c9fbf758756874dc936d5de9812492e6700468a4cd888ef9888c0e8b7f01857",
        ),
        (
            "momento_temp",
            242,
            305,
            "3d603312aa37b36063bedafe94398ef49dd1454e3c227d361695cbd07fd8585f",
        ),
        (
            "momento_shad",
            300,
            300,
            "3a169fa10d58ee2c72828e22f5802bc21cf4763c053414a4d7e94128bb428305",
        ),
        (
            "escape_button1",
            132,
            28,
            "a67a65885d8cfbda4e9b9728863bd0e23ead0aa845346ec442852acd92805371",
        ),
        (
            "escape_button2",
            132,
            28,
            "4d85966e9d55b0493282c0519c49325d5edc8dcd9a95807b8b3c29f01b24788c",
        ),
        (
            "escape_button3",
            132,
            28,
            "2f694946f94cd0a10ddd7acb0e9073226f7994901e2cb753d78921c26e0e4bd8",
        ),
        (
            "daglove",
            40,
            40,
            "434b4dcce0e9df48ee4509c00b8d2867d1417bb59fdc3b4cf5c1ac101b46d726",
        ),
        (
            "dafinger",
            40,
            40,
            "8a65577ac9bf00475544c93175f024c8c2b96fdb3cf10420d82e1dca1b2c4b07",
        ),
        (
            "escapemenu",
            640,
            480,
            "7509ec1d78e833250ba64f51e499d17389ed1ef4f9d16e2f7f15cd6cd0a0e8a0",
        ),
    ];
    for (name, width, height, digest) in textures {
        let image = texture(name);
        assert_eq!((image.width, image.height), (width, height), "{name}");
        assert_eq!(image.sha256, digest, "{name}");
        assert!(image.decodable, "{name}");
    }
    let artwork = screens().ui_artwork("pin6").expect("pin6 decodes");
    assert_eq!((artwork.width(), artwork.height()), (76, 80));

    // The front-end container's escape-menu button art (stored extent digests).
    let members = [
        (
            "ASSETS/GRAPHICS/EM_B_RESUME.PNG",
            221,
            168,
            "0ffd7a4831872842a13a890439cfc795acfb22d12f7a8e2ce05ff44c7e943324",
        ),
        (
            "ASSETS/GRAPHICS/EM_B_LOADGAME.PNG",
            0,
            0,
            "04a659d7e806eca66950417005991d9460136446e70c13c2dd7c41a96596fa94",
        ),
        (
            "ASSETS/GRAPHICS/EM_B_SAVEGAME.PNG",
            0,
            0,
            "30581f316b3cf3c34bef9796c5edec53b865ab02c3b75c84be17396b98d151fe",
        ),
        (
            "ASSETS/GRAPHICS/PC_MAPPINS.PNG",
            50,
            100,
            "89ebe3a87e681926d640f32ae7ce06259545577c6cffd220929b16cfe410a83d",
        ),
    ];
    for (spelling, width, height, digest) in members {
        let image = container_image(spelling);
        if width != 0 {
            assert_eq!((image.width, image.height), (width, height), "{spelling}");
        }
        assert_eq!(image.sha256, digest, "{spelling}");
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity3_pause_map_01_cockpit_map_member_and_what_is_not_referenced() {
    // (g) `map.zrd` of M01 is the cockpit map object, not the Esc chart.
    let map = document(M01_READER, "map.zrd");
    let record = field(&map, "MAP");
    assert_eq!(text(field(record, "MAP_NAME")), "map_c3m01");
    assert_eq!(text(field(record, "OPEN_ANIM")), "open_map");
    assert_eq!(text(field(record, "CLOSE_ANIM")), "close_map");
    assert_eq!(
        floats(field(record, "MAP_DB_SCALE")),
        [8.138_021e-5, 0.0001, 8.138_021e-5]
    );
    assert_eq!(
        floats(field(record, "MAP_DB_POSTSCALE_OFFSET")),
        [0.5, 0.0, 0.5]
    );
    let icon = field(field(record, "ICONS"), "ICON");
    assert_eq!(text(field(icon, "ICON_NAME")), "player_icon");
    assert_eq!(text(field(icon, "DB_NAME")), "player");

    // `map_anims.zrd` opens and closes it by rotating `MAIN_ROOT_NODE` 60 degrees.
    let anims = document(COMMON_READER, "map_anims.zrd");
    let mut names = Vec::new();
    texts(&anims, &mut names);
    for keyword in [
        "open_map",
        "close_map",
        "MAIN_ROOT_NODE",
        "OBJECT_MOTION_FROM_TO",
        "ON_CALL",
    ] {
        assert!(names.contains(&keyword), "map_anims.zrd lacks {keyword}");
    }

    // Measured absences: no document names the 640x480 `escapemenu` or the
    // container's `PC_MAPPINS`; `escape.zrd`, `ia_escape.zrd` and `Briefing.zrd`
    // name no `EM_B_*` art. `Loading.zrd`'s own copy of the dialog names
    // `EM_B_Resume2`..`4`, which no texture of `rimage.zbd` carries.
    for member in ["escape.zrd", "ia_escape.zrd", "Loading.zrd", "Briefing.zrd"] {
        let doc = document(COMMON_READER, member);
        let mut names = Vec::new();
        texts(&doc, &mut names);
        let em_b: Vec<_> = names
            .iter()
            .filter(|name| name.to_ascii_lowercase().starts_with("em_b_"))
            .collect();
        for name in &names {
            let lower = name.to_ascii_lowercase();
            assert!(
                lower != "escapemenu" && !lower.contains("mappins"),
                "{member} names {name}"
            );
        }
        if member == "Loading.zrd" {
            assert_eq!(em_b.len(), 3, "Loading.zrd's EM_B_Resume2..4");
            for name in em_b {
                let known = inventory()
                    .iter()
                    .any(|image| image.name.eq_ignore_ascii_case(name));
                assert!(!known, "{name} unexpectedly exists");
            }
        } else {
            assert!(em_b.is_empty(), "{member} names {em_b:?}");
        }
    }
    let (layout, _) = screens().layout_text().expect("the layout reads");
    let layout = layout.to_ascii_uppercase();
    for needle in ["ESCAPE", "PAUSE", "EM_B_"] {
        assert!(!layout.contains(needle), "LAYOUT.CSV mentions {needle}");
    }
    // `PC_mappins.png` is the Passenger Cabin screen's own pins (`PC_PIN0..4`).
    assert_eq!(layout.matches("PC_MAPPINS").count(), 5);
    let cabin = layout
        .find("[@PASSENGERCABIN@]")
        .expect("the cabin section");
    assert!(layout.find("PC_PIN0").expect("a cabin pin row") > cabin);
}
