# Build the small release APK of Document Slate (phones only, sideload).
#
# How it differs from build-android.ps1 (the everyday debug build):
#   - Only arm64-v8a (real phones). No x86_64 emulator library, so roughly half the size.
#   - Rust is compiled with the "release-small" profile from Cargo.toml: optimised for size
#     ("z"), whole-program link-time optimisation ("fat" LTO) and 1 codegen unit. That makes the
#     .so about 40% smaller, but a full compile takes several minutes instead of ~1.
#   - Gradle's release build: native library stored compressed in the APK, Java shrunk by R8,
#     signed with the upload key if android\keystore.properties exists, else the debug key
#     (debug key = fine for sideloading, rejected by the Play Store).
#
# Usage:  .\build-release.ps1          -> APK for sideloading
#         .\build-release.ps1 -Bundle  -> also an Android App Bundle (.aab), the format Play wants
# Result: android\app\build\outputs\apk\release\app-release.apk
#         android\app\build\outputs\bundle\release\app-release.aab   (with -Bundle)
param([switch]$Bundle)
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot      # run from the folder this script lives in (DocumentSlate\)

# Where the Android SDK and NDK live (the NDK has the C toolchain/linker for Android).
$env:ANDROID_HOME = "$env:LOCALAPPDATA\Android\Sdk"
$env:ANDROID_NDK_HOME = "$env:ANDROID_HOME\ndk\30.0.16248370"

# Use a separate Cargo build folder, so this slow build doesn't overwrite or lock the
# everyday one in target\ (both can then stay cached).
$env:CARGO_TARGET_DIR = "$PSScriptRoot\target-size"

# Privacy: Rust bakes source file paths into error messages, which would put this PC's user
# folder (and so the Windows user name) inside the published APK. --remap-path-prefix rewrites
# them to neutral names. When several match, the LAST matching rule wins, so the general
# user-folder rule comes first. CARGO_ENCODED_RUSTFLAGS separates flags with the 0x1F character
# (plain RUSTFLAGS splits on spaces, and the project path has one).
$repo = Split-Path $PSScriptRoot
$env:CARGO_ENCODED_RUSTFLAGS = @(
    "--remap-path-prefix=$env:USERPROFILE=/home"
    "--remap-path-prefix=$env:USERPROFILE\.cargo=/cargo"
    "--remap-path-prefix=$repo=/src"
) -join [char]0x1f
# Same for C code some dependencies compile (their __FILE__ strings).
$env:CFLAGS = "-ffile-prefix-map=$env:USERPROFILE=/home"

# Step 1: compile Rust into libdocument_slate.so for arm64 phones.
#   cargo ndk            = cargo, but set up with the NDK's compiler/linker for Android
#   -t arm64-v8a         = the CPU type of (almost) every current phone
#   --platform 26        = oldest Android version supported (8.0), same as minSdk in Gradle
#   -o android\release-jniLibs = copy the finished .so to android\release-jniLibs\arm64-v8a\,
#                          which build.gradle.kts uses for the release APK only
#   build --profile release-small --lib = build just the library with the small profile
cargo ndk -t arm64-v8a --platform 26 -o android\release-jniLibs build --profile release-small --lib
if ($LASTEXITCODE -ne 0) { throw "cargo ndk failed" }

$so = Get-Item android\release-jniLibs\arm64-v8a\libdocument_slate.so
"{0,-28} {1,7:N1} MB" -f "libdocument_slate.so:", ($so.Length / 1MB)

# Step 2: package the APK. assembleRelease = build the "release" variant from build.gradle.kts.
#   --offline = use only what's already in the Gradle cache (no downloads)
Push-Location android
#   bundleRelease = the same variant packed as an .aab (Play builds per-device APKs from it)
$tasks = @("assembleRelease"); if ($Bundle) { $tasks += "bundleRelease" }
.\gradlew.bat --offline @tasks
if ($LASTEXITCODE -ne 0) { Pop-Location; throw "gradle failed" }
Pop-Location

# Step 3: report sizes.
#   APK size      = what you copy to the phone.
#   Installed     ~ APK + the unpacked .so (Android extracts compressed native libraries
#                   at install time), shown in Settings > Apps > Document Slate > Storage.
$apk = Get-Item android\app\build\outputs\apk\release\app-release.apk
"{0,-28} {1,7:N1} MB" -f "app-release.apk:", ($apk.Length / 1MB)
"{0,-28} {1,7:N1} MB" -f "installed size (approx.):", (($apk.Length + $so.Length) / 1MB)
"APK: $($apk.FullName)"
if ($Bundle) {
    $aab = Get-Item android\app\build\outputs\bundle\release\app-release.aab
    "{0,-28} {1,7:N1} MB" -f "app-release.aab:", ($aab.Length / 1MB)
    "AAB: $($aab.FullName)"
}
if (-not (Test-Path android\keystore.properties)) { "NOTE: signed with the DEBUG key - not accepted by Play." }
