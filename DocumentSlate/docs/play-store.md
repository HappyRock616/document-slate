# Play Store release notes (checked 2026-11-18)

## Already compliant
- targetSdk 37 (Play requires 36 for new apps and updates from Aug 31, 2026).
- Native library 16 KB page aligned (NDK r30; required for updates from Feb 1, 2027).
- One permission: USE_BIOMETRIC (normal, no prompt) for the digital-ID unlock prompt. No INTERNET.
  File/photo access via SAF and Photo Picker.
- Keep screen on = window flag `FLAG_KEEP_SCREEN_ON` (Java `setKeepScreenOn`), only while a
  document is open and the preference is on (off by default). No permission, not a wake lock;
  Android drops it when the app leaves the screen. The "excessive partial wake locks" vital
  (enforced since Mar 1, 2026) counts screen-off partial wake locks only, so it can't trigger.

## Build for Play
1. Create an upload key (once, keep it OUTSIDE the repo and back it up):
   `keytool -genkeypair -v -keystore <path>\upload-key.jks -alias upload -keyalg RSA -keysize 4096 -validity 10000`
2. Create `android/keystore.properties` (git-ignored) with storeFile, storePassword, keyAlias,
   keyPassword (see the comment at the top of `android/app/build.gradle.kts`).
3. `.\build-release.ps1 -Bundle` -> `android/app/build/outputs/bundle/release/app-release.aab`.
4. In Play Console, enroll in Play App Signing (Google holds the app signing key; you upload
   with the upload key, which can be reset if lost).
5. Raise `versionCode` in `android/app/build.gradle.kts` for every upload.

## Play Console answers
- **Privacy policy URL:** host `docs/PRIVACY.md` as a public web page (not a PDF), fill in
  date + contact email first. Same text is summarized in the app's About page.
- **Data safety:** "Does your app collect or share any of the required user data types?" -> **No**.
  (Collection = data sent off the device; the app has no internet access.) Encryption in transit
  and deletion request questions then don't apply.
- **Ads:** No ads.
- **App access:** All functionality available without special access (no login).
- **Content rating (IARC questionnaire):** category Utility/Productivity; no violence, sexual
  content, gambling, user-to-user communication or location sharing -> expect Everyone / PEGI 3.
- **Target audience:** 13+ (selecting under-13 age groups pulls in the Families policy; the app
  would pass, but it adds review steps).
- **Government / financial / health features:** none.

## Account requirements
- Personal developer accounts created after Nov 13, 2023: closed test with at least 12 testers
  opted in for 14 consecutive days before production access. Organization accounts are exempt.
- Developer verification for sideloaded installs goes worldwide in 2027; Play-distributed
  builds are covered by the Play account.

## Listing cautions
- "Word" is a Microsoft trademark for word processors: complaint risk for the "Word Slate" name.
  Describe formats ("opens .docx, .xlsx, .pptx, .pdf"), never "Microsoft Word compatible" or
  Office logos/screenshots.
- No "ArtCraft"/"-Craft" names anywhere (brand license of the upstream engines).

## Sources
- https://developer.android.com/google/play/requirements/target-sdk
- https://developer.android.com/guide/practices/page-sizes
- https://android-developers.googleblog.com/2026/03/battery-technical-quality-enforcement.html
- https://support.google.com/googleplay/android-developer/answer/10787469
- https://support.google.com/googleplay/android-developer/answer/14151465
- https://www.androidauthority.com/android-sideloading-changes-timeline-3679204/
