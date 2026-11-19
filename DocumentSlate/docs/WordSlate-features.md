# WordSlate — edit-mode features (approved 2026-11-17)

Picked from upstream WordCraft's ribbon (`WordSlate/crates/ui-egui/src/ribbon/`).
**v1** = build first. **Later** = optional, after v1 works.

## Always visible (edit-mode top bar)
Done · Undo · Redo · Save · Search · Print

## Long-press menu on selected text
Cut · Copy · Paste · Paste as plain text · Select all

## Text tab
- **v1:** font, size (+ grow/shrink), bold, italic, underline, strikethrough, superscript/subscript,
  font colour, highlight, clear formatting, change case, styles (Normal, Title, Heading 1–3, Quote)
- **Later:** text effects, small caps, character border

## Paragraph tab
- **v1:** align left/centre/right/justify, bullets, numbering, indent/outdent, line spacing, space before/after
- **Later:** multi-level lists, shading, borders, sort

## Insert tab
- **v1:** picture (gallery/camera), table, link, page break, page numbers, header & footer, comment, symbol
- **Later:** footnotes, shapes, text box, equation, table of contents

## Review tab
- **v1:** spelling (upstream offline checker `craft-proof` + language packs), word count, find & replace,
  comments (next/delete), track changes on/off, accept/reject
- **Later:** thesaurus, compare, protect with password, read aloud

## File tab
- **v1:** save, save as (DOCX, ODT, TXT), export PDF, share, print, page setup (size, orientation, margins),
  document info
- **Later:** templates

## Viewer mode (default, not an edit tab)
Zoom · dark page · outline/navigation · read mode

## Dropped
- Upstream "Agents" group / Commands control channel (automation bridge AI tools can drive) — no-AI rule
- Dictate — keyboard apps already offer voice typing
- Mailings / mail merge
- Most of References: citations, bibliography, index, captions
- Macros, Window, Draw/Ink, Zotero

## Privacy
Upstream has an update checker (`updates_ui.rs`), Zotero integration and language-pack downloads — all use
the network. Plan: update checks off, Zotero off, language packs download only when the user taps to fetch one.
