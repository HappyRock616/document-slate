"""Build Document Slate launcher icon assets (master SVGs + Android PNGs) from one design."""
import os, sys
from playwright.sync_api import sync_playwright

OUT = sys.argv[1]
BG, CHALK, STROKE, GRAIN = "#2B2F33", "#D9DDE0", 8, 0.14

FILTERS = """
<filter id="grain" x="0" y="0" width="100%" height="100%">
 <feTurbulence type="fractalNoise" baseFrequency="0.85" numOctaves="3" seed="7" result="n"/>
 <feColorMatrix in="n" type="matrix" values="0 0 0 0 1  0 0 0 0 1  0 0 0 0 1  0 0 0 0.55 0"/>
</filter>
<filter id="mottle" x="0" y="0" width="100%" height="100%">
 <feTurbulence type="fractalNoise" baseFrequency="0.011" numOctaves="2" seed="3" result="m"/>
 <feColorMatrix in="m" type="matrix" values="0 0 0 0 1  0 0 0 0 1  0 0 0 0 1  0 0 0 0.9 -0.35"/>
</filter>
<mask id="labelcut" maskUnits="userSpaceOnUse" x="-200" y="-200" width="1000" height="1000">
 <rect x="-200" y="-200" width="1000" height="1000" fill="#fff"/>
 <rect x="288" y="331" width="78" height="38" rx="10" fill="#000"/>
</mask>
"""

def slate(x, y, w, h):
    return f"""<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="{BG}"/>
<rect x="{x}" y="{y}" width="{w}" height="{h}" filter="url(#mottle)" opacity="0.10"/>
<rect x="{x}" y="{y}" width="{w}" height="{h}" filter="url(#grain)" opacity="{GRAIN}"/>
<rect x="{x}" y="{y}" width="{w}" height="{h/2}" fill="#ffffff" opacity="0.025"/>"""

TILES = """<g fill="#ffffff" fill-opacity="0.05">
<rect x="96" y="96" width="150" height="150" rx="30"/><rect x="266" y="96" width="150" height="150" rx="30"/>
<rect x="96" y="266" width="150" height="150" rx="30"/><rect x="266" y="266" width="150" height="150" rx="30"/></g>"""

def art(color):
    return f"""<g fill="none" stroke="{color}" stroke-width="{STROKE}" stroke-linecap="round" stroke-linejoin="round">
<path d="M135 125 H185 L207 147 V217 H135 Z"/><path d="M185 125 V147 H207"/>
<path d="M150 165 H192"/><path d="M150 183 H192"/><path d="M150 201 H176"/>
<rect x="295" y="133" width="92" height="76" rx="8"/><path d="M326 133 V209"/><path d="M357 133 V209"/>
<path d="M295 158 H387"/><path d="M295 184 H387"/>
<rect x="123" y="305" width="96" height="62" rx="8"/><path d="M171 367 V385"/><path d="M153 389 H189"/>
<path d="M139 353 L157 337 L174 346 L203 323"/>
<g mask="url(#labelcut)"><path d="M305 295 H355 L377 317 V387 H305 Z"/><path d="M355 295 V317 H377"/></g>
<rect x="292" y="335" width="70" height="30" rx="8"/></g>
<text x="327" y="357" text-anchor="middle" font-family="Inter, sans-serif" font-size="21" font-weight="800"
 letter-spacing="1.5" fill="{color}">PDF</text>"""

def svg(viewbox, body):
    return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{viewbox}" width="512" height="512"><defs>{FILTERS}</defs>{body}</svg>'

# Legacy full icon: rounded slate square with writing-slate frame
legacy = svg("0 0 512 512", f"""<clipPath id="sq"><rect width="512" height="512" rx="120"/></clipPath>
<g clip-path="url(#sq)">{slate(0,0,512,512)}</g>
<rect x="22" y="22" width="468" height="468" rx="100" fill="none" stroke="#fff" stroke-opacity="0.07" stroke-width="3"/>
<rect x="1.5" y="1.5" width="509" height="509" rx="119" fill="none" stroke="#000" stroke-opacity="0.35" stroke-width="3"/>
{TILES}{art(CHALK)}""")

# Legacy round icon
roundi = svg("0 0 512 512", f"""<clipPath id="ci"><circle cx="256" cy="256" r="256"/></clipPath>
<g clip-path="url(#ci)">{slate(0,0,512,512)}</g>
<circle cx="256" cy="256" r="254.5" fill="none" stroke="#000" stroke-opacity="0.35" stroke-width="3"/>
{TILES}{art(CHALK)}""")

# Adaptive icon layers: 108dp canvas; the 512 artwork maps to the central 72dp (viewBox 768, offset -128)
VB = "-128 -128 768 768"
background = svg(VB, slate(-128, -128, 768, 768))
foreground = svg(VB, TILES + art(CHALK))
monochrome = svg(VB, art("#FFFFFF"))

os.makedirs(OUT, exist_ok=True)
masters = {"master/document-slate-icon.svg": legacy, "master/document-slate-icon-round.svg": roundi,
           "master/adaptive-background.svg": background, "master/adaptive-foreground.svg": foreground,
           "master/adaptive-monochrome.svg": monochrome}
for p, s in masters.items():
    os.makedirs(os.path.dirname(os.path.join(OUT, p)), exist_ok=True)
    open(os.path.join(OUT, p), "w").write(s)

DPI = {"mdpi": 1, "hdpi": 1.5, "xhdpi": 2, "xxhdpi": 3, "xxxhdpi": 4}
jobs = []
for d, k in DPI.items():
    res = f"android/res/mipmap-{d}"
    jobs += [(legacy, f"{res}/ic_launcher.png", int(48*k)), (roundi, f"{res}/ic_launcher_round.png", int(48*k)),
             (foreground, f"{res}/ic_launcher_foreground.png", int(108*k)),
             (background, f"{res}/ic_launcher_background.png", int(108*k)),
             (monochrome, f"{res}/ic_launcher_monochrome.png", int(108*k))]
jobs += [(legacy, "png/document-slate-icon-512.png", 512), (legacy, "png/document-slate-icon-1024.png", 1024)]

with sync_playwright() as pw:
    b = pw.chromium.launch(executable_path="/usr/bin/google-chrome")
    pg = b.new_page()
    for s, path, px in jobs:
        pg.set_viewport_size({"width": px, "height": px})
        html = ("<html><body style='margin:0;background:transparent'>"
                + s.replace('width="512" height="512"', f'width="{px}" height="{px}"', 1) + "</body></html>")
        pg.set_content(html)
        pg.wait_for_timeout(50)
        full = os.path.join(OUT, path)
        os.makedirs(os.path.dirname(full), exist_ok=True)
        pg.screenshot(path=full, omit_background=True, clip={"x": 0, "y": 0, "width": px, "height": px})
    b.close()

xml = """<?xml version="1.0" encoding="utf-8"?>
<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
    <background android:drawable="@mipmap/ic_launcher_background" />
    <foreground android:drawable="@mipmap/ic_launcher_foreground" />
    <monochrome android:drawable="@mipmap/ic_launcher_monochrome" />
</adaptive-icon>
"""
d = os.path.join(OUT, "android/res/mipmap-anydpi-v26")
os.makedirs(d, exist_ok=True)
for n in ("ic_launcher.xml", "ic_launcher_round.xml"):
    open(os.path.join(d, n), "w").write(xml)
print("done", len(jobs), "pngs")
