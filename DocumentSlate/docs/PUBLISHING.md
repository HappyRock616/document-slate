# Publishing: GitHub and F-Droid

Status (2026-11-19): GitHub done: public repository, signed releases (`v0.1.1` latest), `main`
protected. F-Droid: first submission closed (its inclusion policy wants about 3 months of active
development first; read the whole policy before resubmitting). Retry around February 2027.

## 0. Before every upload

1. Privacy scan: no names, user paths, device serials, emails or keys in tracked files.
   ```powershell
   git grep --cached -niIE "<your name>|<windows user>|<phone serial>" -- . ':!*.lock'
   git ls-files | Select-String -Pattern "keystore|\.jks|local\.env|local\.properties|CLAUDE"
   ```
   Both must print nothing.
2. The APK too: Rust bakes source paths into error messages. `build-release.ps1` rewrites them
   (`--remap-path-prefix`); check the result still holds no Windows user name:
   ```powershell
   Select-String -Path DocumentSlate\android\release-jniLibs\arm64-v8a\libdocument_slate.so -Pattern "<windows user>" -SimpleMatch -List
   ```
   Must print nothing. (Common first names in the spelling dictionary are expected.)
3. Commits use the neutral identity "Document Slate" <documentslate@invalid.local> (set in this repo's
   git config).

## 1. GitHub

The code lives on GitHub only (GitLab mirror dropped 2026-11-18; can be added later). `gh` is
installed and signed in. First upload:

```powershell
git commit -m "Document Slate 0.1.0"
gh repo create document-slate --public --source . --remote origin --push
git tag v0.1.0
git push origin v0.1.0
gh release create v0.1.0 --title "Document Slate 0.1.0" --notes-file fastlane\metadata\android\en-US\changelogs\1.txt DocumentSlate\android\app\build\outputs\apk\release\app-release.apk#document-slate-0.1.0.apk
```

Later releases: raise `versionCode`/`versionName` in `build.gradle.kts`, add
`changelogs/<versionCode>.txt`, run `build-release.ps1`, scan, commit, tag `v<version>`, push, and
`gh release create` as above. The APK must be signed with the release key (never the debug key):
`apksigner verify --print-certs` must show `CN=Document Slate`.

### Who can change the code

Public = everyone can read; only the owner (and invited collaborators: none) can push. Anyone else
can only *propose* changes (a pull request) that do nothing until you accept them.
Extra safety: Settings → Branches (or Rules) → protect `main`: block force pushes and deletion.

## 2. F-Droid

F-Droid builds the app **from source on its own servers** and signs it, so:

- **The repository must be public.** F-Droid can't build private code. GitHub is fine.
- **Submitting needs a free GitLab account** (F-Droid's own repos live on GitLab), only for the
  merge request; the code doesn't have to be there.
- **Each release needs a git tag** (`v0.1.0`) and a higher `versionCode` in
  `android/app/build.gradle.kts`.
- **Store page** comes from `fastlane/metadata/android/en-US/` in this repo (title, descriptions,
  icon, changelog per versionCode). Phone screenshots still needed, 2-8 PNGs in
  `images/phoneScreenshots/`: take them with test files, not personal ones.

### Already done in the code

- No Google Play Services, Firebase, ads or analytics. No INTERNET permission.
- Google's encrypted dependency report turned off (`dependenciesInfo` in `build.gradle.kts`).
- `-Punsigned` builds an unsigned release APK for F-Droid to sign.
- No prebuilt binaries in the source tree (apart from the standard Gradle wrapper).
- Licenses: MIT OR Apache-2.0 (code), OFL (fonts), Apache-2.0 (dictionaries).
- Upstream ArtCraft names and logos removed from the app and saved files.

### Submitting

1. Public GitHub repo, tag `v0.1.0`, push.
2. Fork https://gitlab.com/fdroid/fdroiddata, add `metadata/org.documentslate.app.yml` (draft
   below), open a merge request. F-Droid's reviewers usually help fix the build recipe.

### Draft `metadata/org.documentslate.app.yml` (untested)

```yaml
Categories:
  - Writing
  - Reading
License: Apache-2.0
SourceCode: https://github.com/HappyRock616/document-slate
IssueTracker: https://github.com/HappyRock616/document-slate/issues

AutoName: Document Slate

RepoType: git
Repo: https://github.com/HappyRock616/document-slate.git

Builds:
  - versionName: 0.1.0
    versionCode: 1
    commit: v0.1.0
    subdir: DocumentSlate/android/app
    submodules: true
    rm:
      # ArtCraft trademark files (not open source, never used by the app)
      - WordSlate/docs/brand
      - GridSlate/docs/brand
      - DeckSlate/docs/brand
      - PageSlate/docs/brand
    srclibs:
      - rustup@1.28.2
    gradle:
      - yes
    gradleprops:
      - unsigned
    ndk: 30.0.16248370
    build:
      - $$rustup$$/rustup-init.sh -y --profile minimal --default-toolchain stable
      - source $HOME/.cargo/env
      - rustup target add aarch64-linux-android
      - cargo install cargo-ndk --version 4.1.2 --locked
      - pushd ../..
      - cargo ndk -t arm64-v8a --platform 26 -o android/release-jniLibs build --profile release-small --lib
      - popd

AutoUpdateMode: Version
UpdateCheckMode: Tags
CurrentVersion: 0.1.0
CurrentVersionCode: 1
```

Notes:
- The license is dual (MIT OR Apache-2.0); ask the reviewers whether to list it as `Apache-2.0` or
  the full expression.
- The Rust build fetches crates and the pinned egui/winit forks from GitHub during the build,
  which F-Droid allows for source code.
- `rm:` paths are relative to the repo root in fdroiddata (not `subdir`); reviewers may adjust.
- Network code inside the engines (font language-pack downloader `ureq`) is compiled in but can't
  run: the app has no INTERNET permission. OCR models are not bundled (OCR not in the app yet).

### Signing: F-Droid key or yours?

By default F-Droid signs with **its own key**. Then an F-Droid install and a Play/sideload install
can't update each other (Android refuses a different signature).

To ship **your** signature on F-Droid, the build has to be *reproducible*: F-Droid rebuilds the
app, checks it matches your signed APK byte for byte, then publishes yours
(`AllowedAPKSigningKeys` + `Binaries` in the metadata). Rust release builds can be made
reproducible but it takes extra work; start with F-Droid's key and switch later.
