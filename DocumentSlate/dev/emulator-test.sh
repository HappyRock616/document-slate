#!/usr/bin/env bash
# Build Document Slate, install it on the running emulator/phone, start it, take a screenshot.
# Stops at the first error, so a failed build never installs an old APK.
#   usage (Git Bash):  dev/emulator-test.sh [screenshot-name]      e.g. dev/emulator-test.sh home1
# Screenshots go to ../_cleanup/ (git-ignored scratch folder).
set -euo pipefail
cd "$(dirname "$0")/.."

export PATH="$HOME/.cargo/bin:$PATH"
export ANDROID_HOME="$LOCALAPPDATA/Android/Sdk"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/30.0.16248370"
export MSYS_NO_PATHCONV=1   # stop Git Bash rewriting /sdcard/... into a Windows path
ADB="$ANDROID_HOME/platform-tools/adb.exe"
SHOT_DIR="../_cleanup"
mkdir -p "$SHOT_DIR"

echo "== Rust (desktop check)"
cargo build 2>&1 | grep -E '^(error|warning)|^\s+-->|Finished' || true
cargo build -q 2>/dev/null

echo "== Rust (Android)"
cargo ndk -t x86_64 -t arm64-v8a --platform 26 -o android/app/src/main/jniLibs build --release --lib -q

echo "== APK"
(cd android && ./gradlew.bat assembleFullDebug --offline -q 2>&1 | grep -vi deprecat || true)
test -f android/app/build/outputs/apk/full/debug/app-full-debug.apk

echo "== Install + launch"
"$ADB" install -r android/app/build/outputs/apk/full/debug/app-full-debug.apk | tail -1
"$ADB" shell am force-stop org.documentslate.app
"$ADB" logcat -c
"$ADB" shell am start -n org.documentslate.app/.MainActivity >/dev/null
sleep 8
echo "pid=$("$ADB" shell pidof org.documentslate.app || echo 'NOT RUNNING')"
"$ADB" logcat -d | grep -E 'RustPanic|panicked|AndroidRuntime' | grep -v uwb | tail -5 || true

if [ -n "${1:-}" ]; then
    "$ADB" exec-out screencap -p > "$SHOT_DIR/$1.png"
    echo "screenshot: $SHOT_DIR/$1.png"
fi
