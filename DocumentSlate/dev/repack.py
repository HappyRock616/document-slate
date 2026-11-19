"""Swap a freshly built Rust library into the last Gradle-built debug APK and re-sign it.

Why: Gradle needs local network connections (127.0.0.1) between its client and daemon. When a
VPN/firewall blocks those, Gradle can't build at all. As long as only Rust code changed (no Java,
manifest or resources), the APK just needs its native library replaced:

  1. copy every entry of app-debug.apk except lib/<abi>/libdocument_slate.so (and old signatures),
  2. add the new .so from android/app/src/main/jniLibs/<abi>/,
  3. zipalign it (Android requires aligned APKs),
  4. sign it with the standard Android debug key (~/.android/debug.keystore, password "android").

usage:  python -I dev/repack.py  [out.apk]      (run from DocumentSlate/)
"""
import os
import subprocess
import sys
import zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SDK = os.path.join(os.environ["LOCALAPPDATA"], "Android", "Sdk")
TOOLS = os.path.join(SDK, "build-tools", "36.0.0")
SRC = os.path.join(ROOT, "android", "app", "build", "outputs", "apk", "debug", "app-debug.apk")
OUT = sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, "android", "app", "build", "outputs", "apk", "debug", "app-repacked.apk")
UNALIGNED = OUT + ".unaligned"
ABIS = ["arm64-v8a", "x86_64"]

def main():
    replaced = []
    with zipfile.ZipFile(SRC) as zin, zipfile.ZipFile(UNALIGNED, "w") as zout:
        for item in zin.infolist():
            name = item.filename
            if name.startswith("META-INF/") and name.split("/")[-1].upper().endswith((".SF", ".RSA", ".EC", ".MF")):
                continue  # old signature: re-signed below
            if name.endswith("/libdocument_slate.so"):
                continue  # replaced below
            zout.writestr(item, zin.read(name))
        for abi in ABIS:
            so = os.path.join(ROOT, "android", "app", "src", "main", "jniLibs", abi, "libdocument_slate.so")
            # Only ABIs the original APK had, so an emulator-only .so doesn't sneak into a phone build.
            if os.path.exists(so) and any(i.filename == f"lib/{abi}/libdocument_slate.so" for i in zin.infolist()):
                zout.write(so, f"lib/{abi}/libdocument_slate.so", compress_type=zipfile.ZIP_DEFLATED)
                replaced.append(abi)
    subprocess.run([os.path.join(TOOLS, "zipalign.exe"), "-f", "-p", "4", UNALIGNED, OUT], check=True)
    os.remove(UNALIGNED)
    ks = os.path.join(os.path.expanduser("~"), ".android", "debug.keystore")
    subprocess.run(
        [os.path.join(TOOLS, "apksigner.bat"), "sign", "--ks", ks, "--ks-pass", "pass:android", "--key-pass", "pass:android", "--ks-key-alias", "androiddebugkey", OUT],
        check=True,
        shell=False,
    )
    print(f"repacked {OUT} (new library for: {', '.join(replaced) or 'none!'}) {os.path.getsize(OUT) / 1048576:.1f} MB")

if __name__ == "__main__":
    main()
