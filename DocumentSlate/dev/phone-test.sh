#!/usr/bin/env bash
# Build Document Slate for a real phone (arm64 only), install it, start it, and optionally take a
# screenshot. Stops at the first error.
# Builds a RELEASE APK signed with the release key (android/keystore.properties), so it installs
# over the GitHub/Obtainium copy on the phone. No debug builds.
# Faster than build-release.ps1: plain release profile (thin LTO) instead of release-small.
# Device: set PHONE=<serial from 'adb devices'> or put it in dev/local.env (git-ignored).
#   usage (Git Bash):  dev/phone-test.sh [screenshot-name]
# Flavor: "full" (GitHub/F-Droid, in-app folder lists) by default; FLAVOR=play for the Play build.
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
FLAVOR="${FLAVOR:-full}"
case "$FLAVOR" in full) TASK=assembleFullRelease ;; play) TASK=assemblePlayRelease ;; *) echo "FLAVOR must be full or play"; exit 1 ;; esac
mkdir -p "$SHOT_DIR"

echo "== Rust (Android arm64)"
cargo ndk -t arm64-v8a --platform 26 -o android/release-jniLibs build --release --lib -q

echo "== APK ($FLAVOR release, release key)"
[ -f android/keystore.properties ] || { echo "android/keystore.properties missing: would sign with the debug key"; exit 1; }
(cd android && ./gradlew.bat "$TASK" --offline -q 2>&1 | { grep -vi deprecat || true; })

echo "== Install + launch on $PHONE"
"$ADB" -s "$PHONE" install -r "android/app/build/outputs/apk/$FLAVOR/release/app-$FLAVOR-release.apk" | tail -1
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
