# Roadmap

Status as of 2026-11-19: version 0.1.0 released. All four tools run in one app and have been tested on a real phone.

## Done (first draft)

- **Suite**: start screen with the four apps, a screen per app (New, Recent, Browse), preferences
  drawer (Dark, Keep screen on, Recent, spelling language, author name, About), opens almost any
  file (unknown text files open in WordSlate), viewer first with auto-hiding bars, page counter,
  Search, Share, Print to local network printers, Save / Save as, full-fidelity save guard,
  upstream names removed from saved file metadata.
- **WordSlate**: view, read mode, outline, dark page; edit tabs Text, Paragraph, Insert, Review
  (spelling, tracking, comments), File (DOCX/ODT/TXT/PDF, paper, margins); tables, pictures,
  shapes and charts editable.
- **GridSlate**: sheet view with frozen panes and charts; tabs Cell, Formula, Data, Insert, File;
  opens xlsx, xls, xlsb, ods, csv.
- **DeckSlate**: viewer, slide strip editor, slideshow, shapes gallery with full styling; tabs
  Slides, Text, Insert, Design, File (pptx/ppsx, PDF slides/handouts/notes).
- **PageSlate**: viewer, thumbnails, night mode; Annotate, Edit, Fill & Sign (saved signatures),
  Pages, File; password-protected PDFs; incremental saves keep signatures valid; certificate
  signatures (digital IDs in the Android Keystore, self-signed or imported .p12/.pfx), Save
  flattened.
- **Links and barcodes (all tools)**: tap a link, QR code or barcode in a document to see what it
  holds and open web addresses in the default browser; Insert › Barcode makes QR, Data Matrix,
  Aztec, PDF417, Code 128/39, EAN-13 and UPC-A.

## Before the first release

- [x] Signed release build measured against the 150 MB target: APK 41 MB, installed ~128 MB.
- [ ] Print to a real Wi-Fi printer (all four apps). Deferred: no printer paired with the test phone.
- [x] Privacy policy: date and contact (GitHub issues) filled in; published with the repository.
- [x] PDF test `fill_and_sign_marks`: typed text is an annotation until flattened (test fixed).
      Engine quirk left: text extraction can split a flattened word, so search may miss it.
- [x] Real-phone check: keyboard autocorrect (typed on the on-screen keyboard: "teh" → "the",
      "adn" → "and"), app resume after screen lock (fingerprint unlock).
- [x] Privacy scan (files and APK), first commit, public GitHub repository, release `v0.1.0`.
- [ ] F-Droid: 2-8 phone screenshots (test files only),
      merge request to fdroiddata (see [PUBLISHING.md](PUBLISHING.md)).

## Later

**GridSlate**: CSV import options, ODS save, numeric keyboard, edit charts and pictures.

**DeckSlate**: cut/copy/paste, multi-select, comments, background pictures, slideshow pen and
laser, .odp support.

**PageSlate**: merge / insert pages from another PDF, drag to reorder pages, crop by dragging,
select and copy text, restyle existing comments, OCR.

**Suite**: smaller app size (align duplicate engine library versions), four launcher icons,
.doc and .ppt saving (engine limits).
