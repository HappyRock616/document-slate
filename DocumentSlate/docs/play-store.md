# Play Store release notes (checked 2026-11-18)

## Already compliant
- targetSdk 37 (Play requires 36 for new apps and updates from Aug 31, 2026).
- Native library 16 KB page aligned (NDK r30; required for updates from Feb 1, 2027).
- One permission (`play` flavor): USE_BIOMETRIC (normal, no prompt) for the digital-ID unlock
  prompt. No INTERNET. File/photo access via SAF and Photo Picker. (The `full` flavor adds
  MANAGE_EXTERNAL_STORAGE; it is not uploaded to Play.)
- Keep screen on = window flag `FLAG_KEEP_SCREEN_ON` (Java `setKeepScreenOn`), only while a
  document is open and the preference is on (off by default). No permission, not a wake lock;
  Android drops it when the app leaves the screen. The "excessive partial wake locks" vital
  (enforced since Mar 1, 2026) counts screen-off partial wake locks only, so it can't trigger.

## Build for Play
Play gets the **`play` flavor**: no storage permission, folder tabs open Android's file picker.
(The `full` flavor, with "All files access", is for GitHub and F-Droid; see the section below
before ever uploading it to Play.)
1. Create an upload key (once, keep it OUTSIDE the repo and back it up):
   `keytool -genkeypair -v -keystore <path>\upload-key.jks -alias upload -keyalg RSA -keysize 4096 -validity 10000`
2. Create `android/keystore.properties` (git-ignored) with storeFile, storePassword, keyAlias,
   keyPassword (see the comment at the top of `android/app/build.gradle.kts`).
3. `.\build-release.ps1 -Flavor play -Bundle` ->
   `android/app/build/outputs/bundle/playRelease/app-play-release.aab`.
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

## Permissions Declaration (All files access)
Only needed if the `full` flavor (with `MANAGE_EXTERNAL_STORAGE`) is ever uploaded to Play.
Today Play gets the `play` flavor, which has no storage permission and needs no declaration.
Document management is one of the uses Google allows for "All files access"; the declaration form
asks for the core feature that needs it and a short video of it.

**Core feature:** document management. Document Slate is an office suite (documents,
spreadsheets, presentations, PDF). Its start screen and each app's screen have folder tabs
(Downloads, Documents, Pictures, Camera, Movies, Music) that list the documents in those folders
inside the app, so the user can open a file and save it back in place.

**Justification text** (paste into the form):

> Document Slate is an offline office suite: it opens, edits and saves documents, spreadsheets,
> presentations and PDFs where they already are on the phone. Its main screen lists the
> documents in the phone's standard folders (Download, Documents, Pictures, DCIM, Movies, Music)
> so users can find and open them in one tap, as in a desktop office suite.
>
> The Storage Access Framework cannot do this. Since Android 11 the system picker refuses to grant
> access to the Download folder as a whole (ACTION_OPEN_DOCUMENT_TREE blocks it), and Download is
> where most documents arrive, from browsers, email and messaging apps. Without All files access
> the app cannot show what is in Download at all; it can only hand users the system picker for
> one file at a time, and every saved copy needs another picker trip. MediaStore does not help
> either: it only returns other apps' non-media files (.docx, .xlsx, .pdf…) with this
> permission.
>
> Access is used only for that list and for open/save of the files the user taps. The app lists
> only those six shared folders and their subfolders; it never reads Android/, hidden folders or
> other apps' private data, and it never scans in the background (a folder is read only while
> its list is on screen). The permission is optional: until the user turns it on in Android's
> settings, the app explains what it is for and offers the system picker instead.
>
> The app has no INTERNET permission, so no file can leave the phone through it. It has no ads,
> analytics or accounts and collects no data.

**Video to record:** start screen → tap Downloads → the "Allow access" card → Android's All files
access switch → back in the app, the Downloads list → open a document → edit → Save → the list
shows it changed.

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
- https://support.google.com/googleplay/android-developer/answer/10467955 (All files access)
- https://developer.android.com/training/data-storage/shared/documents-files#document-tree-access-restrictions
