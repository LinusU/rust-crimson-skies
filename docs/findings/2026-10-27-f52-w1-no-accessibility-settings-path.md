# F52-W1: the accessibility settings file path (resolved)

Task #780 (F52-W1) opens `SettingsSession::open(path, safe_defaults)` at boot.
No path was established for the file, so the first session blocked instead of
inventing one. The owner decided it on 2026-10-29. This is a design decision,
not a claim about the original game, which stored nothing here.

## Decision

1. **File name:** `accessibility.settings`. The contents are the existing
   `cs_content::settings` text that `accessibility::store` reads and writes.
   The extension names our format and implies no original file.
2. **Scope:** per user, directly under the user-data base:
   `<user_data::default_base>/accessibility.settings`. Not per profile:
   accessibility has to apply from the first front-end frame before any profile
   is chosen, the settings are the player's comfort settings on this machine
   rather than save state, and the profile settings document is a different
   format.
3. **`--profile-dir <dir>`:** an automated run uses
   `<dir>/accessibility.settings` and never touches the player's real base.

## Implementation

- `user_data::accessibility_settings_path(base)` is the one place the name is
  chosen; `user_data::resolve_profile_dir` returns the validated `--profile-dir`.
- `cli::open_accessibility_session` opens the session from an explicit base or
  the platform base; `cli::settings_origin_message` reports its `StartupOrigin`,
  naming the reason when a file was recovered.
- `--safe-settings` is parsed by `cli::parse` and belongs to `--playtest` only.
  The synthetic runs read no accessibility settings, and every other run mode
  rejects the flag with exit 2.

## Limits

- Nothing consumes the session yet: the playtest holds it for the run, reports
  its origin and flushes it at teardown. The presenters and the control profile
  reach it through F52-W2-wire (#2140) and F52-W3.
- Only `--playtest` opens the session. The start flow, front end and mission
  runs do not yet; they open it when they gain a consumer.
