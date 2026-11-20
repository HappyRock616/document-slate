# Document Slate: Privacy Policy

*Effective date: November 20, 2026*

Document Slate is a free office suite for Android (documents, spreadsheets, slides and PDFs).
It has no ads, no analytics, no accounts and no AI features.

## The short version

**Document Slate does not collect, store on any server, sell or share any of your data.**
The app cannot connect to the internet: it does not request Android's internet permission.

## What stays on your device

- **Your files.** Documents you open, edit or create stay on your device or in the storage
  location you pick (for example a cloud folder offered by Android's file picker). Document
  Slate reads and writes only the files you choose through Android's file picker, photo picker
  or "Open with".
- **App settings.** Your preferences (dark mode, screen on, recent files list, spelling
  language, your name) are kept in the app's private storage on your device. Uninstalling the
  app deletes them. The recent files list can be turned off and cleared in the app.
- **Your name (optional).** If you enter your name in the preferences, it is written into the
  author field of documents you create or save, as other office apps do. It goes wherever
  you send those documents. Leave the field empty if you don't want this.
- **Saved signatures.** Signatures you save in the PDF tools are kept in the app's private
  storage on your device.
- **Digital IDs.** Certificate keys for signing PDFs are kept in the Android Keystore on your
  device and never leave it, except as a backup file you choose to save yourself.

## When data leaves the app

Only when you choose to, using Android's own system features:

- **Share:** Android's share sheet sends a copy of the document to the app you pick.
- **Print:** Android's print system sends the document to the printer or print service you
  pick. That service's own privacy policy applies.
- **Save to a location:** files saved to a cloud folder are handled by that provider.

Document Slate itself never sends anything anywhere.

## Permissions

Document Slate requests one Android permission, "use biometric hardware", so it can show
Android's own fingerprint / face / PIN prompt before your digital ID signs a document. The app
never sees your fingerprint, face or PIN; Android only tells it "confirmed" or "cancelled".
It has no internet permission. File and photo access goes through Android's system pickers,
which give the app access only to the items you select.

The version from GitHub and F-Droid can also ask for "All files access" (optional, off until
you turn it on in Android's settings). With it, the folder tabs list the documents in your
phone's shared folders (Download, Documents, Pictures, DCIM, Movies, Music) inside the app, and
files open and save in place. The app reads those folders only while their list is on screen;
it never reads other apps' private data and never scans in the background. The Google Play
version does not have this permission.

## Children

Document Slate collects no data from anyone, including children.

## Changes

If this policy changes, the new version will be published at this address with a new
effective date.

## Contact

Questions: open an issue at https://github.com/HappyRock616/document-slate/issues
