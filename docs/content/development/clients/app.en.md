+++
title = "Flutter App"
weight = 300
[extra]
source_file_hash = "914b1bc76162e7171829d26c93ed2d49a132ff69"
translated_at = "2026-10-25T00:00:00Z"
+++

# Flutter App

The Flutter client (`apps/app`) is currently a **skeleton**: no vanling business features are wired up yet. Cross-client semantics live in [Shared Client Protocol](@/development/clients/shared-protocol.en.md); page structure lives in [Page Implementation](@/development/clients/pages.en.md).

## Current capabilities

| Capability | Status |
| --- | --- |
| Theme, adaptive layout (incl. desktop detection), text scaling, i18n | Present |
| Network layer (Dio) / database (Sqlite) / event bus / unique IDs (nanoid2) | Present |
| Environment config (dev / prod) | Present |
| Auth (Spring-authorization-server) | Present |
| Auto-update | Android only |
| Logging | Rotating log files (except on Web); export/upload not implemented |
| Release | Android and Web present; iOS, Windows (portable exe), Linux, macOS absent |
| User profile / integration tests / CI code-quality and tests | Not implemented |

## Requirements

| Tool | Notes |
| --- | --- |
| Xcode | iOS/macOS builds; afterwards `sudo xcode-select --switch /Applications/Xcode.app` |
| Android Studio | Manages the Android SDK; install SDK API 34/35 on first launch |
| Chrome | Web debugging via `flutter run -d chrome` |

The toolchain ships with `nix develop`; once inside the shell, `flutter test` / `flutter analyze` / `flutter build apk` all work.

First run: `nix develop` → `flutter doctor --android-licenses` → `flutter pub get` → `flutter doctor` to confirm.

## Releasing on Android

1. Create a keystore

   ```shell
   keytool -genkey -v -keystore .\android-app-keystore.jks -storetype JKS -keyalg RSA -keysize 2048 -validity 10000 -alias android-app
   ```

2. Create `[project]/android/key.properties` pointing at it

   ```properties
   storePassword=<password from step 1>
   keyPassword=<password from step 1>
   keyAlias=android-app
   storeFile=<keystore path>
   ```

3. Build (production flavour)

   ```shell
   flutter build apk --dart-define=DART_DEFINE_APP_ENV=prod
   ```

## Coding conventions

- Run `dart run build_runner build` after changing JSON models.
- Sqlite3 on Web needs its binary first: `dart run sqflite_common_ffi_web:setup` writes `sqlite3.wasm` and `sqflite_sw.js` into `web/`; re-run with `--force` after upgrading sqlite3.
- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/).

## Coordinate systems

The server stores coordinates as WGS84; the client renders with AMap (GCJ-02) and converts via `coordtransform_dart`. Any cross-boundary coordinate read or write must convert here — do not assume the other side's datum.

## Known issues

If `pod install` is slow, `cd ios/` and run `pod install --verbose`; export `https_proxy` / `http_proxy` / `all_proxy` if a proxy is needed.
