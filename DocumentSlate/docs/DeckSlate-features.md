# DeckSlate — edit-mode features (approved 2026-11-17)

Picked from upstream DeckCraft's ribbon (`DeckSlate/crates/ui-egui/src/ribbon/`).
Edit tabs: **Slide · Text · Insert · Design · File** (plus a contextual object-editing tab).
**v1** = build first. **Later** = optional, after v1 works.

## Always visible (edit-mode top bar)
Done · Undo · Redo · Save · Play (start slideshow) · Print

## Slide strip
Thumbnails; long-press to duplicate, delete, hide, move.

## Long-press menu on a selected object
Cut · Copy · Paste · Duplicate · Delete · Bring to front / Send to back

## Slide tab
- **v1:** new slide (pick layout), duplicate, delete, hide, change layout, reset slide, speaker notes
- **Later:** sections, slide sorter view

## Text tab
- **v1:** font, size, bold, italic, underline, font colour, align (left/centre/right, top/middle/bottom),
  bullets, numbering, list level up/down, line spacing, clear formatting
- **Later:** character spacing, text columns, text effects

## Insert tab
- **v1:** text box, picture, shapes (basic set), table, link, slide number / date / footer, comment
- **Later:** chart, icons (bundled set), video/audio, symbol, equation, diagram (SmartArt)

## Design tab
- **v1:** themes, theme colours/fonts, background (colour or picture), slide size (16:9 / 4:3)
- **Later:** Design Ideas (offline theme/layout previews — not AI), simple transitions (fade, push, wipe),
  simple entrance animations

## Object editing (contextual, when a picture/shape/table is tapped)
- **v1:** move/resize/rotate by finger, fill and outline colour, crop picture, table add/delete rows and
  columns, merge cells, align/distribute
- **Later:** picture corrections/effects, shape effects, group/ungroup

## File tab
- **v1:** save, save as (PPTX, ODP), export PDF, share, print (slides, or handouts with several per page)
- **Later:** export slide as image

## Slideshow (Play)
- **v1:** full screen, swipe/tap forward and back, laser pointer/pen, exit
- **Later:** presenter view with notes and timer (for casting to a TV)

## Viewer mode (default)
Swipe through slides · zoom · Play

## Dropped
- Translate (disabled placeholder needing a language model)
- Cameo / narration recording (camera, microphone)
- `mcp` crate (AI/automation bridge)
- Slide master editing, handout/notes masters
- Ink drawing tools, photo album, screenshot
- Compare presentations, multiple windows

## Privacy
No network code in DeckSlate. Only the automation bridge to remove.
