#!/usr/bin/env bash
# Build Document Slate for a real phone (arm64 only, quick debug APK), install it, start it,
# and optionally take a screenshot. Stops at the first error.
# Device: set PHONE=<serial from 'adb devices'> or put it in dev/local.env (git-ignored).
#   usage (Git Bash):  dev/phone-test.sh [screenshot-name]
# Screenshots go to ../_cleanup/ (git-ignored scratch folder).
set -euo pipefail
cd "$(dirname "$0")/.."

export PATH="$HOME/.cargo/bin:$PATH"
export ANDROID_HOME="$LOCALAPPDATA/Android/Sdk"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/30.0.16248370"
export MSYS_NO_PATHCONV=1
ADB="$ANDROID_HOME/platform-tools/adb.exe"
[ -f dev/local.env ] && . dev/local.env
PHONE="${PHONE:?set PHONE to your device serial (adb devices), e.g. in dev/local.env}"
SHOT_DIR="../_cleanup"
mkdir -p "$SHOT_DIR"

echo "== Rust (Android arm64)"
cargo ndk -t arm64-v8a --platform 26 -o android/app/src/main/jniLibs build --release --lib -q

echo "== APK"
(cd android && ./gradlew.bat assembleDebug --offline -q 2>&1 | { grep -vi deprecat || true; })

echo "== Install + launch on $PHONE"
"$ADB" -s "$PHONE" install -r android/app/build/outputs/apk/debug/app-debug.apk | tail -1
"$ADB" -s "$PHONE" shell am force-stop org.documentslate.app
"$ADB" -s "$PHONE" logcat -c
"$ADB" -s "$PHONE" shell am start -n org.documentslate.app/.MainActivity >/dev/null
sleep 5
echo "pid=$("$ADB" -s "$PHONE" shell pidof org.documentslate.app || echo 'NOT RUNNING')"
"$ADB" -s "$PHONE" logcat -d | grep -E 'RustPanic|panicked|AndroidRuntime' | tail -5 || true

if [ -n "${1:-}" ]; then
    "$ADB" -s "$PHONE" exec-out screencap -p > "$SHOT_DIR/$1.png"
    echo "screenshot: $SHOT_DIR/$1.png"
fi
