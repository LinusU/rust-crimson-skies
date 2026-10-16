//! Acceptance tests for task #636 (`M01-LC-WEATHER-BIND`): `weather.zrd`
//! becomes an `EnvironmentSession` input through production code, and every
//! field it does not bind is named.

use std::path::PathBuf;

use cs_app::environment::{
    EnvironmentSession, RetailWeather, RunSeeds, VisibilityUnavailable, WindUnavailable,
    read_mission_weather,
};
use cs_assets::install;
use cs_content::environment::PrecipitationKind;
use cs_content::stunts::ZrdValue;
use cs_content::weather::{
    PrecipitationType, WeatherDocument, WeatherError, WeatherScalar, bind_weather,
};
use cs_sim::time::TickRate;
use cs_types::asset_id::SourceSpan;
use cs_types::content::Resolved;

const M01: &str = "zbd/c1c/m01";

fn text(value: &str) -> ZrdValue {
    ZrdValue::Text(value.to_owned())
}

fn fl(value: f32) -> ZrdValue {
    ZrdValue::Float(value)
}

fn list(items: Vec<ZrdValue>) -> ZrdValue {
    ZrdValue::List(items)
}

fn floats(values: &[f32]) -> ZrdValue {
    list(values.iter().copied().map(fl).collect())
}

fn fields(pairs: Vec<(&str, ZrdValue)>) -> ZrdValue {
    list(
        pairs
            .into_iter()
            .flat_map(|(key, value)| [text(key), value])
            .collect(),
    )
}

fn zone() -> ZrdValue {
    zone_colored(floats(&[0.5, 0.5, 0.5]))
}

fn zone_colored(fog_color: ZrdValue) -> ZrdValue {
    fields(vec![
        ("FOG_COLOR", fog_color),
        ("FOG_RANGES", floats(&[100.0, 200.0])),
        (
            "FOG_ALTITUDE",
            list(vec![ZrdValue::Int(0), ZrdValue::Int(10)]),
        ),
        ("CLIP_RANGES", floats(&[5.0, 300.0])),
        ("SUNLIGHT_ACTIVE", list(vec![ZrdValue::Int(1)])),
        (
            "SUNLIGHT_ORIENTATION",
            list(vec![ZrdValue::Int(4_294_967_271), fl(90.0), fl(0.0)]),
        ),
        ("SUNLIGHT_DIFFUSE", floats(&[1.0])),
        ("SUNLIGHT_AMBIENT", floats(&[0.25])),
        ("SUNLIGHT_COLOR_DIFFUSE", floats(&[1.0, 1.0, 1.0])),
        ("SUNLIGHT_COLOR_AMBIENT", floats(&[1.0, 1.0, 1.0])),
        ("SUNLIGHT_STATIC", list(vec![ZrdValue::Int(1)])),
        ("SUNLIGHT_BICOLORED", list(vec![ZrdValue::Int(0)])),
    ])
}

fn tree(extra: Vec<(&str, ZrdValue)>) -> ZrdValue {
    tree_with_zones(vec![("ZONE1", zone()), ("SW_ZONE1", zone())], extra)
}

fn tree_with_zones(zones: Vec<(&str, ZrdValue)>, extra: Vec<(&str, ZrdValue)>) -> ZrdValue {
    let tier = || {
        fields(vec![
            ("CLIP_SCALE", floats(&[1.0])),
            ("FOG_SCALE", floats(&[1.0])),
        ])
    };
    let mut pairs = vec![
        (
            "VIEWING_RANGE",
            fields(vec![("HIGH", tier()), ("SWHIGH", tier())]),
        ),
        (
            "WIND",
            fields(vec![
                ("STATIC_VELOCITY", floats(&[0.0, 2.0, 0.0])),
                ("RANDOM_MAX_SPEED", fl(10.0)),
                ("RANDOM_ACCEL", fl(5.0)),
                ("RANDOM_ANG_VEL", fl(5.0)),
            ]),
        ),
        (
            "CLOUD_COVER",
            fields(vec![
                ("TOP", fl(1100.0)),
                ("BOTTOM", fl(1000.0)),
                ("THICKNESS", fl(30.0)),
            ]),
        ),
    ];
    pairs.extend(zones);
    pairs.push(("SHADOW_ANGLES", floats(&[-90.0, 0.0, 0.0])));
    pairs.extend(extra);
    list(vec![fields(pairs)])
}

fn rain() -> Vec<(&'static str, ZrdValue)> {
    vec![
        ("TYPE", text("RAIN")),
        ("PARTICLES", ZrdValue::Int(100)),
        ("COLOR", list(vec![ZrdValue::Int(128); 3])),
        ("WIND_DIR", fl(0.0)),
        ("WIND_VEL", fl(0.8)),
        ("GRAVITY", fl(3.0)),
        ("ALPHA_GRADIENT", floats(&[0.5, 0.0])),
    ]
}

fn span() -> SourceSpan {
    SourceSpan::new(
        install::sha256(b"install"),
        "zbd/c1c/m01/zrdr.zbd",
        Some("weather.zrd"),
        0,
        10,
        None,
    )
    .expect("the synthetic span is recordable")
}

fn session(binding: &cs_content::weather::WeatherBinding) -> EnvironmentSession {
    EnvironmentSession::new(
        binding.definition.clone(),
        TickRate::new(64).expect("64 Hz is a tick rate"),
        RunSeeds::from_root(1),
    )
    .expect("an authored definition starts a session")
}

#[test]
fn accept_m01_lc_weather_bind_document_is_read_strictly() {
    let document = WeatherDocument::from_zrd(&tree(rain())).expect("the synthetic tree parses");
    assert_eq!(document.zones.len(), 2);
    assert_eq!(document.zones[0].name, "ZONE1");
    assert_eq!(document.zones[1].name, "SW_ZONE1");
    assert_eq!(document.viewing_range.len(), 2);
    let precipitation = document.precipitation.expect("TYPE RAIN is present");
    assert_eq!(precipitation.kind, PrecipitationType::Rain);
    // The two spellings of one field are both kept.
    assert_eq!(
        document.zones[0].fog_altitude.map(|a| a[0]),
        Some(WeatherScalar::Int(0))
    );
    assert_eq!(document.zones[0].fog_ranges[0], WeatherScalar::Float(100.0));
    assert!((document.zones[0].sunlight_orientation[0].signed() + 25.0).abs() < 1e-9);

    let unknown = WeatherDocument::from_zrd(&tree(vec![("GUST", fl(1.0))]));
    assert!(matches!(
        unknown,
        Err(WeatherError::UnknownKey { ref key, .. }) if key == "GUST"
    ));
    let duplicate = WeatherDocument::from_zrd(&tree(vec![("SHADOW_ANGLES", floats(&[0.0; 3]))]));
    assert!(matches!(duplicate, Err(WeatherError::DuplicateKey { .. })));
    let mut partial = rain();
    partial.pop(); // ALPHA_GRADIENT
    assert!(matches!(
        WeatherDocument::from_zrd(&tree(partial)),
        Err(WeatherError::MissingKey {
            key: "ALPHA_GRADIENT",
            ..
        })
    ));
    let mut bad = rain();
    bad[0] = ("TYPE", text("HAIL"));
    assert!(matches!(
        WeatherDocument::from_zrd(&tree(bad)),
        Err(WeatherError::UnknownPrecipitationType { .. })
    ));
}

#[test]
fn accept_m01_lc_weather_bind_binds_only_what_the_file_decides() {
    let document = WeatherDocument::from_zrd(&tree(rain())).expect("parses");
    let binding = bind_weather(M01, &document, &span()).expect("binds");
    let mut running = session(&binding);

    let definition = running.definition();
    assert!(definition.origin().is_original());
    match definition.precipitation().kind() {
        Resolved::Known(known) => assert_eq!(known.value, PrecipitationKind::Rain),
        Resolved::Unknown { .. } => panic!("TYPE RAIN decides the kind"),
    }
    // Everything unit- or convention-dependent stays unknown, and the
    // consumers say so instead of returning still air or a fog-derived range.
    assert!(matches!(
        running.wind(),
        Err(WindUnavailable::Unknown { .. })
    ));
    assert!(matches!(
        running.sight_range_m(),
        Err(VisibilityUnavailable::Unknown { .. })
    ));
    assert!(!definition.sun_direction().is_known());
    assert!(!definition.sky_orientation().is_known());
    assert!(!definition.fog().density_per_m().is_known());

    // What the file does decide binds known: `CLOUD_COVER.BOTTOM` is the
    // layer's floor altitude in measured metres (task #1460), and the one
    // hardware-path zone's `FOG_COLOR` is the fog colour, already
    // normalized because every channel sits at or below 1.0.
    let layers = definition.cloud_layers();
    assert_eq!(layers.len(), 1, "CLOUD_COVER binds one cloud layer");
    assert_eq!(layers[0].altitude_m(), 1000.0, "BOTTOM is a metre altitude");
    assert!(
        !layers[0].coverage().is_known(),
        "the record authors no coverage fraction"
    );
    let Resolved::Known(fog_color) = definition.fog().color_linear() else {
        panic!("one hardware-path zone decides the fog colour");
    };
    assert_eq!(fog_color.value, [0.5, 0.5, 0.5]);
    running
        .advance_frame(std::time::Duration::from_nanos(15_625_000))
        .expect("the weather clock advances");

    let keys: Vec<&str> = binding.unbound.iter().map(|f| f.key).collect();
    for expected in [
        "WIND.STATIC_VELOCITY",
        "CLOUD_COVER.TOP/THICKNESS",
        "ZONEn.FOG_RANGES/FOG_ALTITUDE",
        "ZONEn.SUNLIGHT_ORIENTATION",
        "ZONEn.CLIP_RANGES",
        "SHADOW_ANGLES",
        "PARTICLES/COLOR/WIND_DIR/WIND_VEL/GRAVITY/ALPHA_GRADIENT",
    ] {
        assert!(keys.contains(&expected), "{expected} must be named unbound");
    }
    assert!(
        !keys.contains(&"ZONEn.FOG_COLOR"),
        "FOG_COLOR binds when the hardware-path zones agree"
    );
}

/// The measured scale on the record: `BOTTOM` binds the layer's floor in
/// metres, a `FOG_COLOR` channel above 1.0 is byte-scaled by 1/255 (the
/// rule measured at 0x4bc350), and a fog colour the zones disagree on
/// stays an explicit unknown rather than picking one zone's.
#[test]
fn accept_m01_lc_weather_bind_measured_units_and_colour_scale() {
    // One hardware-path zone, mixed spellings: the integer 51 normalizes
    // to 51/255 while the float 0.5 passes through verbatim.
    let mixed = WeatherDocument::from_zrd(&tree_with_zones(
        vec![(
            "ZONE1",
            zone_colored(list(vec![ZrdValue::Int(51), fl(0.5), ZrdValue::Int(255)])),
        )],
        vec![],
    ))
    .expect("the synthetic tree parses");
    let binding = bind_weather(M01, &mixed, &span()).expect("binds");
    let Resolved::Known(fog) = binding.definition.fog().color_linear() else {
        panic!("a single hardware-path zone decides the fog colour");
    };
    assert_eq!(
        fog.value,
        [51.0 / 255.0, 0.5, 1.0],
        "channels above 1.0 are byte-scaled, the rest pass verbatim"
    );
    let layer = &binding.definition.cloud_layers()[0];
    assert_eq!(layer.altitude_m(), 1000.0);

    // Two hardware-path zones that disagree: the contract holds one fog
    // colour, so it stays unknown and the field is named.
    let disagreeing = WeatherDocument::from_zrd(&tree_with_zones(
        vec![
            ("ZONE1", zone_colored(floats(&[0.5, 0.5, 0.5]))),
            ("ZONE2", zone_colored(floats(&[0.6, 0.6, 0.6]))),
        ],
        vec![],
    ))
    .expect("the synthetic tree parses");
    let binding = bind_weather(M01, &disagreeing, &span()).expect("binds");
    assert!(
        !binding.definition.fog().color_linear().is_known(),
        "disagreeing zones must not pick a winner"
    );
    assert!(
        binding.unbound.iter().any(|f| f.key == "ZONEn.FOG_COLOR"),
        "the disagreement must be named unbound"
    );

    // The SW_ twin is the software-render path: it does not take part in
    // the agreement the hardware-path zones reach.
    let twin_differs = WeatherDocument::from_zrd(&tree_with_zones(
        vec![
            ("ZONE1", zone_colored(floats(&[0.5, 0.5, 0.5]))),
            ("SW_ZONE1", zone_colored(floats(&[0.9, 0.9, 0.9]))),
        ],
        vec![],
    ))
    .expect("the synthetic tree parses");
    let binding = bind_weather(M01, &twin_differs, &span()).expect("binds");
    let Resolved::Known(fog) = binding.definition.fog().color_linear() else {
        panic!("an SW_ twin's colour must not break the hardware agreement");
    };
    assert_eq!(fog.value, [0.5, 0.5, 0.5]);
}

#[test]
fn accept_m01_lc_weather_bind_absent_precipitation_is_inferred_clear() {
    let document = WeatherDocument::from_zrd(&tree(vec![])).expect("parses");
    assert!(document.precipitation.is_none());
    let binding = bind_weather(M01, &document, &span()).expect("binds");
    match binding.definition.precipitation().kind() {
        Resolved::Known(known) => {
            assert_eq!(known.value, PrecipitationKind::Clear);
            assert_eq!(
                known.provenance.class,
                cs_types::evidence::ClaimStatus::Inferred
            );
        }
        Resolved::Unknown { .. } => panic!("absence is bound as inferred clear air"),
    }
    assert!(
        !binding
            .unbound
            .iter()
            .any(|f| f.key.starts_with("PARTICLES"))
    );
}

fn retail_root() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must name the original installation"),
    )
}

fn weather_of(found: &install::Discovery, key: &str) -> RetailWeather {
    read_mission_weather(found, key).unwrap_or_else(|error| panic!("{key}: {error}"))
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_weather_bind_retail_m01_yields_an_environment_input() {
    let found = install::discover(&retail_root()).expect("the installation is discoverable");
    let weather = weather_of(&found, M01);
    assert_eq!(weather.member_bytes, 3429);
    assert_eq!(weather.document.zones.len(), 4);
    assert_eq!(
        weather.document.precipitation.as_ref().map(|p| p.kind),
        Some(PrecipitationType::Rain)
    );
    assert_eq!(
        weather
            .document
            .wind
            .static_velocity
            .map(WeatherScalar::as_f64),
        [0.0, 2.0, 0.0]
    );
    let running = weather
        .session(TickRate::new(64).expect("64 Hz"), RunSeeds::from_root(7))
        .expect("M01's weather starts a session");
    assert!(running.definition().origin().is_original());
    assert!(matches!(
        running.wind(),
        Err(WindUnavailable::Unknown { .. })
    ));
    assert!(
        weather
            .binding
            .unbound
            .iter()
            .any(|field| field.key == "WIND.STATIC_VELOCITY")
    );
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_weather_bind_retail_corpus_all_missions_bind() {
    let found = install::discover(&retail_root()).expect("the installation is discoverable");
    let mut missions = Vec::new();
    for record in &found.manifest.files {
        let key = record.relative_spelling.logical_key();
        if let Some(mission) = key.strip_suffix("/zrdr.zbd")
            && mission.split('/').count() == 3
        {
            missions.push(mission.to_owned());
        }
    }
    missions.sort();
    let (mut rain, mut snow, mut clear, mut with_zone3) = (0, 0, 0, 0);
    for mission in &missions {
        let weather = weather_of(&found, mission);
        match weather.document.precipitation.as_ref().map(|p| p.kind) {
            Some(PrecipitationType::Rain) => rain += 1,
            Some(PrecipitationType::Snow) => snow += 1,
            None => clear += 1,
        }
        if weather.document.zones.iter().any(|z| z.name == "ZONE3") {
            with_zone3 += 1;
        }
        assert!(!weather.binding.unbound.is_empty());
    }
    assert_eq!(
        missions.len(),
        53,
        "every mission scope carries a weather document"
    );
    assert_eq!((rain, snow, clear, with_zone3), (9, 5, 39, 8));
}
