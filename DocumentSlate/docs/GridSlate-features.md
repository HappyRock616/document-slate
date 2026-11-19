# GridSlate — edit-mode features (approved 2026-11-17)

Picked from upstream GridCraft's ribbon (`GridSlate/crates/ui-egui/src/ribbon/`).
**v1** = build first. **Later** = optional, after v1 works.

## Always visible (edit-mode top bar)
Done · Undo · Redo · Save · Search · Print

## Formula bar
Shows/edits the selected cell's contents, with the cell address (e.g. B4).

## Long-press menu on selected cells
Cut · Copy · Paste · Paste values only · Clear contents · Insert/delete row or column

## Cell tab
- **v1:** font, size, bold, italic, underline, font colour, fill colour, align (left/centre/right,
  top/middle/bottom), wrap text, merge/unmerge, borders (common presets), number formats (number, currency,
  percent, date, comma style), more/fewer decimals, clear formats
- **Later:** cell styles, conditional formatting, rotated text, format painter

## Formula tab
- **v1:** AutoSum (sum, average, count, min, max), insert function by category (maths, text, date, logical,
  lookup, financial), recently used
- **Later:** named ranges, show formulas, error checking / trace precedents, manual calculation mode

## Data tab
- **v1:** sort A→Z / Z→A, custom sort, filter, remove duplicates, freeze top row / first column / panes,
  import CSV
- **Later:** data validation (dropdown lists), text to columns, group/outline, subtotals, flash fill

## Insert tab
- **v1:** rows, columns, sheets (add/delete/rename), chart (column, bar, line, pie), picture, link,
  comment/note
- **Later:** more chart types, sparklines, shapes, PivotTable, header & footer

## File tab
- **v1:** save, save as (XLSX, ODS, CSV), export PDF, share, print, page setup (orientation, paper size,
  margins, fit to page, print area, gridlines on/off)
- **Later:** protect sheet/workbook with password

## Viewer mode (default)
Zoom · sheet tabs · freeze panes kept from the file · dark mode

## Dropped
- Automate tab: Agent Control, command palette, action journal (AI/automation bridge, plus the `mcp` crate)
- Macros (record/run)
- Solver, forecast sheet, what-if analysis
- Ink/drawing, form controls (buttons, sliders, checkboxes), slicers
- Multiple windows

## Privacy
No network code in GridSlate (only git URLs of build dependencies). Nothing to disable beyond the automation bridge.

## Shared code
Uses upstream `craft-ui` and `craft-print` (storytold/craft-libs) — shared with the other apps in the combined APK.
