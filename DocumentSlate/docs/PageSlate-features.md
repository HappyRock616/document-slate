# PageSlate — edit-mode features (approved 2026-11-17)

Picked from upstream PdfCraft (`PageSlate/crates/ui-egui/src/tools/`, `workflows/`) — it uses Acrobat-style
tools and a hotbar rather than a ribbon.
Edit tabs: **Annotate · Edit · Fill & Sign · Pages · File**.
**v1** = build first. **Later** = optional, after v1 works.

## Always visible (top bar)
Back · Undo · Redo · Save · Search · Print

## Viewer mode (default — where most PDF use happens)
Scroll/swipe pages, pinch zoom, page thumbnails, outline/bookmarks, night mode, select & copy text, open links

## Annotate tab
- **v1:** highlight, underline, strikethrough, sticky-note comment, text box, freehand pen,
  shapes (rectangle, circle, arrow, line), comment list (jump to, delete)
- **Later:** stamps, callouts, attach file, measurement tools

## Edit tab
- **v1:** edit existing text, add text, add/move/resize/delete image, add/edit links
- **Later:** find & replace text, redact (incl. find text and redact)

## Fill & Sign tab
- **v1:** fill form fields (text, checkboxes, dropdowns), add text/ticks/crosses to flat forms,
  draw or type signature + date/initials, save signatures for reuse
- **Later:** digital (certificate) signatures, create forms, form calculations
  (PDF JavaScript — keep OFF until we decide how to sandbox it)

## Pages tab
- **v1:** reorder (drag thumbnails), rotate, delete, insert blank page or pages from another PDF,
  extract pages to new PDF, merge PDFs, crop pages
- **Later:** split by page count, page numbers/headers, enhance scanned pages

## File tab
- **v1:** save, save as, share, print, password-protect / remove password, document properties
- **Later:** reduce file size (optimise), export to images/text, create PDF from pictures

## OCR — Later (approved)
Upstream's offline recogniser (`ocrs`, small pre-trained model, MIT/Apache code, CC-BY-SA-4.0 models).
Classic text recognition, runs on the phone, nothing sent anywhere.

## Dropped
- Agent control (`automation` crate — AI/automation bridge)
- "Join the ArtCraft community" link (ArtCraft branding)
- Cloud-folder shortcuts (OneDrive/Dropbox/Google Drive) — Android's file picker covers these
- Accessibility tagging, preflight, compare, actions/JavaScript editor, bulk form tools
- Camera tool (use Android camera via "create PDF from pictures")

## Privacy
No network code in the parts we use. The web build's OCR model download is irrelevant to Android.
