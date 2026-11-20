# Building Document Slate

Windows is the tested build machine. Commands are PowerShell, run from `DocumentSlate/` unless noted.

## 1. Tools (once)

| Tool | Version | Get it |
|------|---------|--------|
| Rust | stable | https://rustup.rs |
| Android targets | — | `rustup target add aarch64-linux-android x86_64-linux-android` |
| cargo-ndk | 4.1.2+ | `cargo install cargo-ndk` |
| Android Studio | current | gives the SDK, `adb` and a JDK |
| Android SDK platform | 37 | Android Studio → SDK Manager |
| Android NDK | 30.0.16248370 | SDK Manager → SDK Tools → NDK (Side by side) |
| JDK | 21 | bundled with Android Studio, or Microsoft OpenJDK 21 |
| Python 3 | optional | only for `dev/repack.py` and test-file generators |

The build scripts expect the SDK at `%LOCALAPPDATA%\Android\Sdk` (Android Studio's default).

## 2. Get the code

```powershell
git clone --recurse-submodules https://github.com/HappyRock616/document-slate.git
```

The four engines (`WordSlate/`, `GridSlate/`, `DeckSlate/`, `PageSlate/`) and the spelling
dictionaries (`craft-i18n/`) are submodules pinned to tested commits. If you cloned without them:

```powershell
git submodule update --init --depth 1
```

Without `craft-i18n/`, spelling still works but falls back to a small built-in English list.

## 3. Debug build (everyday)

```powershell
.\build-android.ps1            # phone (arm64) + emulator (x86_64) debug APK
.\build-android.ps1 -Install   # also install and start it on the connected device
```

Result: `android\app\build\outputs\apk\full\debug\app-full-debug.apk`.
The first build downloads crates and Gradle parts and takes a while; later builds take about a minute.

Desktop test window (no phone needed): `cargo run` opens the app in a phone-sized window.
Unit tests: `cargo test --lib`.

## 4. Release build

Two build flavors (product flavors in `android/app/build.gradle.kts`):

- **`full`** (default): GitHub, F-Droid, sideload. Declares `MANAGE_EXTERNAL_STORAGE` (in
  `android/app/src/full/AndroidManifest.xml`). Once the user turns on "All files access", the
  folder tabs list Download, Documents, Pictures, DCIM, Movies and Music inside the app, and those
  files open and save in place by path. Until then a card offers the switch or the picker.
- **`play`**: Google Play. No storage permission; the folder tabs open Android's file picker.

The Rust library is the same for both: it asks Java (`hasAllFilesFlavor()`, from
`BuildConfig.ALL_FILES`) at start.

```powershell
.\build-release.ps1                       # small arm64 APK, full flavor (GitHub / sideload)
.\build-release.ps1 -Flavor play -Bundle  # Play flavor APK + .aab for the Play Store
```

Uses the `release-small` Cargo profile (size-optimised, full LTO), so it takes several minutes.
Output goes to `android\app\build\outputs\apk\<flavor>\release\app-<flavor>-release.apk` (and
`bundle\<flavor>Release\`).

## 5. Signing

Without a key file, release builds are signed with the debug key — fine for your own phone, not
for distribution.

1. Create an upload key once, **outside the repository**, and back it up:
   ```powershell
   keytool -genkeypair -v -keystore <path>\upload-key.jks -alias upload -keyalg RSA -keysize 4096 -validity 10000
   ```
2. Create `android\keystore.properties` (git-ignored, never commit it):
   ```
   storeFile=C:/path/outside/the/repo/upload-key.jks
   storePassword=...
   keyAlias=upload
   keyPassword=...
   ```
3. Run `.\build-release.ps1` (or `-Flavor play -Bundle` for Play).
4. Raise `versionCode` in `android\app\build.gradle.kts` for every release.

Play Store details: [play-store.md](play-store.md).

## 6. Testing on a phone

Turn on USB debugging on the phone, connect it, and check it shows in `adb devices`.

- `dev/phone-test.sh [shot]` (Git Bash): arm64 release build signed with the release key
  (`full` flavor; `FLAVOR=play dev/phone-test.sh` for Play), install over the phone's copy,
  start, optional screenshot.
  Put `PHONE=<serial from adb devices>` in `dev/local.env` (git-ignored).
- `dev/emulator-test.sh [shot]`: the same for a running emulator.
- Screenshots land in `../_cleanup/` (git-ignored).

## Troubleshooting

- **Gradle or adb hang / "connection refused"**: something is blocking `127.0.0.1` (VPN split
  tunneling or firewall). Gradle and adb both talk to themselves over localhost.
- **Every engine file shows as modified**: run `git config core.filemode false` in that submodule.
- **Emulator shows a black screen**: emulators use OpenGL ES automatically; real devices use Vulkan.
- **After updating an engine submodule**: keep the `[patch]` revisions in `Cargo.toml` in step with
  `WordSlate/Cargo.toml`, and re-fetch `craft-i18n` if `WordSlate/craft-i18n.rev` changed.
