"""Makes the small test workbooks used by slate-grid's unit tests.

They are written by openpyxl (not by GridCraft), so the tests also check that files made by
another program open, edit and save correctly.

Run (from anywhere):
    python -I make_fixture.py <output folder>

Writes:
    foreign.xlsx              2 sheets: numbers, formulas, styles, merge, freeze, note, chart,
                              link, data validation
    foreign_with_slicer.xlsx  the same plus a fake slicer part, which GridCraft does not keep,
                              to test that slate-grid reports the loss.
"""
import os
import sys
import zipfile

from openpyxl import Workbook
from openpyxl.chart import BarChart, Reference
from openpyxl.comments import Comment
from openpyxl.styles import Border, Font, PatternFill, Side
from openpyxl.worksheet.datavalidation import DataValidation

out = sys.argv[1] if len(sys.argv) > 1 else "."
wb = Workbook()
ws = wb.active
ws.title = "Sales"
ws.append(["Fruit", "Qty", "Price", "Total"])
rows = [("Apple", 10, 0.5), ("Banana", 24, 0.25), ("Cherry", 3, 4.0), ("Date", 7, 2.5)]
for i, (name, qty, price) in enumerate(rows, start=2):
    ws.append([name, qty, price, f"=B{i}*C{i}"])
ws["A6"] = "Sum"
ws["D6"] = "=SUM(D2:D5)"
for c in ws[1]:
    c.font = Font(bold=True, color="FFFFFF")
    c.fill = PatternFill("solid", fgColor="2E8B57")
thin = Side(style="thin", color="000000")
ws["D6"].border = Border(top=thin, bottom=Side(style="double", color="000000"))
ws["C2"].number_format = "0.00"
ws.merge_cells("F1:H1")
ws["F1"] = "Merged title"
ws.freeze_panes = "A2"
ws["A2"].comment = Comment("Fresh today", "Alex")
ws["A8"] = "Website"
ws["A8"].hyperlink = "https://example.org"
dv = DataValidation(type="list", formula1='"Yes,No"', allow_blank=True)
ws.add_data_validation(dv)
dv.add("B8")
chart = BarChart()
chart.title = "Totals"
chart.add_data(Reference(ws, min_col=4, min_row=1, max_row=5), titles_from_data=True)
chart.set_categories(Reference(ws, min_col=1, min_row=2, max_row=5))
ws.add_chart(chart, "F3")
ws2 = wb.create_sheet("Notes")
ws2["A1"] = "Second sheet"
ws2["B2"] = "=Sales!D6*2"
plain = os.path.join(out, "foreign.xlsx")
wb.save(plain)

# Same file plus a slicer part (GridCraft drops slicers: we want that reported, not silent).
slicer = os.path.join(out, "foreign_with_slicer.xlsx")
with zipfile.ZipFile(plain) as src, zipfile.ZipFile(slicer, "w", zipfile.ZIP_DEFLATED) as dst:
    for item in src.infolist():
        data = src.read(item.filename)
        if item.filename == "[Content_Types].xml":
            data = data.replace(
                b"</Types>",
                b'<Override PartName="/xl/slicers/slicer1.xml" '
                b'ContentType="application/vnd.ms-excel.slicer+xml"/></Types>',
            )
        dst.writestr(item, data)
    dst.writestr(
        "xl/slicers/slicer1.xml",
        '<?xml version="1.0" encoding="UTF-8"?><slicers '
        'xmlns="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main">'
        '<slicer name="Fruit" cache="Slicer_Fruit" caption="Fruit" rowHeight="241300"/></slicers>',
    )
print("wrote", plain, slicer)
