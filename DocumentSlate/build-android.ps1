# Build the Document Slate APK.
#   1. cargo ndk compiles the Rust code into libdocument_slate.so for each Android CPU type
#      and drops it into android/app/src/main/jniLibs/<abi>/
#   2. Gradle packages that into an APK (android/app/build/outputs/apk/full/debug/app-full-debug.apk)
# Usage:  .\build-android.ps1            (debug, phone + emulator)
#         .\build-android.ps1 -Install   (also install on the running emulator/phone via adb)
param([switch]$Install)
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

$env:ANDROID_HOME = "$env:LOCALAPPDATA\Android\Sdk"
$env:ANDROID_NDK_HOME = "$env:ANDROID_HOME\ndk\30.0.16248370"

# arm64-v8a = real phones/tablets, x86_64 = the emulator on this PC
cargo ndk -t arm64-v8a -t x86_64 --platform 26 -o android/app/src/main/jniLibs build --release --lib
if ($LASTEXITCODE -ne 0) { throw "cargo ndk failed" }

Push-Location android
.\gradlew.bat assembleFullDebug
if ($LASTEXITCODE -ne 0) { Pop-Location; throw "gradle failed" }
Pop-Location

if ($Install) {
    & "$env:ANDROID_HOME\platform-tools\adb.exe" install -r android/app/build/outputs/apk/full/debug/app-full-debug.apk
    & "$env:ANDROID_HOME\platform-tools\adb.exe" shell am start -n org.documentslate.app/.MainActivity
}
