# F52-ORIGINAL-OPTIONS: where the original stores its options, and what is measured

Task: #789 `F52-ORIGINAL-OPTIONS`. Supersedes F52-A limit 1
(`2026-10-01-f52-a-accessibility-settings.md`) and F52-D limit 5
(`2026-10-08-f52-d-accessibility-flows-gpu-review.md`).
Test: `crates/cs_content/tests/accept_f52_original_options.rs` (prefix
`accept_f52_original_options_`, ignored without `CS_ENGINE_IMAGE`).
Provenance of every row below: `static` (bytes of the owner-supplied decrypted
engine image). Nothing here is `verified_original`; no original executable ran.

## Answer

1. **Where.** The original keeps its options in the Windows registry, not in
   the installation. The image holds the key
   `SOFTWARE\Microsoft\Microsoft Games\Crimson Skies\1.0` (offsets `0x21f0d0`,
   `0x21f108`, `0x223a40`) and imports `RegOpenKeyExA` / `RegQueryValueExA`.
   This agrees with F52-D's measurement that the installation holds no
   `*.ini`, `*.cfg` or `*.opt` file. A second key,
   `...\Crimson Skies\Special` (`0x229844`), exists; what it stores is unknown.
2. **Which names.** A contiguous block of NUL-terminated names (below) is the
   engine's config table (the neighbouring source-path string is
   `D:\zipper\Crimson\config.cpp`). They are value or key names; whether each
   is read from the registry, from `detail.zrd` or both is **not measured**.
3. **No subtitle, FOV, shake, flash or UI-scale name** is in that block (the
   test pins this for the block). `FOV` strings elsewhere in the image
   (`After FOV: %d`, `FOV CLIPPED`, `REJECTED FOV`) are renderer clipping
   diagnostics, not a player option. This is absence of a *name in one block*,
   not proof that no such option exists: a name could be built at run time or
   live in resources (`strings.dll`, ZBD data).
4. **Closest matches to F52 settings** (names only, no semantics):
   `MouseSens` (mouse flight sensitivity), `FlyMode`, `JoystickNumAxes`,
   `JoystickNumButtons`, `DefaultView`, `ViewingRange`, `EnemySkill`,
   `GameIntensity`, `Shadow`, `Clutter`, `MusicOn/Volume`, `SfxOn/Volume`,
   `VoiceOn/Volume`, `SoundLOD`, `FullScreen`, `VMode`, `InGameVMode`. The
   owner's installed README additionally names a *Shadows* option in a *Video*
   menu and a *Sound Quality* High/Low option in an *Audio* menu (manual text,
   not committed).

## Name table (first occurrence, `static`)

File offset; virtual address = offset + `0x400000`.

| Name | Offset | VA |
| --- | --- | --- |
| `DEFAULT` | `0x2234e8` | `0x6234e8` |
| `HWCardFlag` | `0x2234fc` | `0x6234fc` |
| `HWCardDev` | `0x223508` | `0x623508` |
| `EffectsLevel_SW` | `0x223514` | `0x623514` |
| `Shadow` | `0x223534` | `0x623534` |
| `Clutter` | `0x223544` | `0x623544` |
| `ViewingRange` | `0x223554` | `0x623554` |
| `EffectsLevel_HW` | `0x223574` | `0x623574` |
| `GfxFlags_SW` | `0x223594` | `0x623594` |
| `Transparency` | `0x2235a0` | `0x6235a0` |
| `Lighting` | `0x2235b0` | `0x6235b0` |
| `Perspective` | `0x2235bc` | `0x6235bc` |
| `GlobalLight_SW` | `0x2235c8` | `0x6235c8` |
| `AllVideoBuffer` | `0x2235d8` | `0x6235d8` |
| `GfxFlags_HW` | `0x2235e8` | `0x6235e8` |
| `GlobalLight_HW` | `0x22361c` | `0x62361c` |
| `ObjectLOD_SW` | `0x22363c` | `0x62363c` |
| `ObjectLOD_HW` | `0x22365c` | `0x62365c` |
| `TextureMemory_SW` | `0x22367c` | `0x62367c` |
| `TextureMemory_HW` | `0x2236a4` | `0x6236a4` |
| `GameIntensity` | `0x2236cc` | `0x6236cc` |
| `AllowIME` | `0x2236dc` | `0x6236dc` |
| `FlyMode` | `0x2236e8` | `0x6236e8` |
| `MouseSens` | `0x2236f0` | `0x6236f0` |
| `AskForVDev` | `0x2236fc` | `0x6236fc` |
| `LoadOptionalSounds` | `0x223708` | `0x623708` |
| `VoiceOn` | `0x223730` | `0x623730` |
| `VoiceVolume` | `0x223738` | `0x623738` |
| `MusicOn` | `0x223744` | `0x623744` |
| `MusicVolume` | `0x223754` | `0x623754` |
| `SfxOn` | `0x223760` | `0x623760` |
| `SfxVolume` | `0x223768` | `0x623768` |
| `SoundLOD` | `0x223774` | `0x623774` |
| `PlayerName` | `0x22378c` | `0x62378c` |
| `UIAssetPath` | `0x223798` | `0x623798` |
| `GOSData` | `0x2237a4` | `0x6237a4` |
| `D3DFlag` | `0x2237ac` | `0x6237ac` |
| `FullScreen` | `0x2237b4` | `0x6237b4` |
| `HUDFlag` | `0x2237c0` | `0x6237c0` |
| `HWAPI` | `0x2237c8` | `0x6237c8` |
| `JoystickNumAxes` | `0x2237d0` | `0x6237d0` |
| `JoystickNumButtons` | `0x2237e0` | `0x6237e0` |
| `Network` | `0x2237f4` | `0x6237f4` |
| `NetListen` | `0x2237fc` | `0x6237fc` |
| `Ammo` | `0x223808` | `0x623808` |
| `Fuel` | `0x223810` | `0x623810` |
| `DefaultView` | `0x223818` | `0x623818` |
| `EnemySkill` | `0x223824` | `0x623824` |
| `PlayerSpeed` | `0x223830` | `0x623830` |
| `MPFuel` | `0x22383c` | `0x62383c` |
| `Render` | `0x223844` | `0x623844` |
| `Display` | `0x22384c` | `0x62384c` |
| `Window` | `0x223854` | `0x623854` |
| `Replicate` | `0x22385c` | `0x62385c` |
| `VMode` | `0x223868` | `0x623868` |
| `InGameVMode` | `0x223878` | `0x623878` |
| `VStride` | `0x223890` | `0x623890` |
| `sunlight` | `0x223898` | `0x623898` |

Resolutions named beside it: `ZVID_320x240x16`, `ZVID_640x480x16`,
`ZVID_800x600x16`, `ZVID_1024x768x16` (`0x223454`..), and strings
`320 x 240`, `640 x 480`, `800 x 600`, `1024 x 768` (`0x223a10`..).

## Not measured (owner-only or later work)

- **Ranges and defaults of every option.** Not recoverable from names. They
  need either decompilation of the config reader (static; not attempted here,
  affects every row) or the owner's registry export after a real run, or the
  original options screens captured by an original run (`human_review`).
- **The player-visible option screens** (labels, order, which names are exposed
  at all). Owner-only.
- **The original FOV.** Unmeasured, so `ModernAssist::Fov` keeps treating any
  explicit FOV as an assist; this task revisits nothing there.

## Claim gates that stay in force

No task may assert that any F52 setting matches an original option. The names
above are evidence that the original has *some* stored mouse sensitivity,
fly mode, view, sound and video settings; they say nothing about values.
Resolving: a registry export from the owner's machine after an original run
(owner supplies), or a later static task on the config reader.
