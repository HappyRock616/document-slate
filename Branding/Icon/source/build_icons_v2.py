"""Document Slate launcher icon, version 2: bigger tiles, bigger symbols, thicker lines.

Two variants, each written to its own folder next to the original (the original is untouched):
  v2-gray/   chalk-gray symbols on the slate (like v1, just bolder)
  v2-color/  each symbol in its app's color (Word blue, Grid green, Deck orange, Page red)

Each folder gets master/*.svg, android/res/mipmap-*/ PNGs (legacy, round, adaptive layers) and
png/ 512 + 1024. PNGs are rendered with headless Microsoft Edge (Windows).

Run:  python -I build_icons_v2.py
"""
import os, subprocess, tempfile, pathlib

HERE = pathlib.Path(__file__).resolve().parent.parent  # Branding/Icon
EDGE = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"

BG, CHALK = "#2B2F33", "#D9DDE0"
APP = {"word": "#2F6DB5", "grid": "#2E8B57", "deck": "#D9732B", "page": "#C2423A"}
STROKE = 14          # v1 was 8
TILE, GAP = 186, 20  # v1 was 150 / 20
T0 = 256 - TILE - GAP / 2          # first tile edge (60)
C1, C2 = T0 + TILE / 2, 256 + GAP / 2 + TILE / 2   # tile centres (153, 359)

FILTERS = """
<filter id="grain" x="0" y="0" width="100%" height="100%">
 <feTurbulence type="fractalNoise" baseFrequency="0.85" numOctaves="3" seed="7" result="n"/>
 <feColorMatrix in="n" type="matrix" values="0 0 0 0 1  0 0 0 0 1  0 0 0 0 1  0 0 0 0.55 0"/>
</filter>
<filter id="mottle" x="0" y="0" width="100%" height="100%">
 <feTurbulence type="fractalNoise" baseFrequency="0.011" numOctaves="2" seed="3" result="m"/>
 <feColorMatrix in="m" type="matrix" values="0 0 0 0 1  0 0 0 0 1  0 0 0 0 1  0 0 0 0.9 -0.35"/>
</filter>"""


def slate(x, y, w, h):
    return f"""<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="{BG}"/>
<rect x="{x}" y="{y}" width="{w}" height="{h}" filter="url(#mottle)" opacity="0.10"/>
<rect x="{x}" y="{y}" width="{w}" height="{h}" filter="url(#grain)" opacity="0.14"/>
<rect x="{x}" y="{y}" width="{w}" height="{h/2}" fill="#ffffff" opacity="0.025"/>"""


def tiles(colors):
    """Four faint tiles. `colors` = per-tile tint (None = plain white tint)."""
    out = []
    for (x, y), c in zip([(T0, T0), (T0 + TILE + GAP, T0), (T0, T0 + TILE + GAP),
                          (T0 + TILE + GAP, T0 + TILE + GAP)], colors):
        fill, op = (c, 0.10) if c else ("#ffffff", 0.05)
        out.append(f'<rect x="{x}" y="{y}" width="{TILE}" height="{TILE}" rx="36" '
                   f'fill="{fill}" fill-opacity="{op}"/>')
    return "\n".join(out)


def g(color, body):
    return (f'<g fill="none" stroke="{color}" stroke-width="{STROKE}" stroke-linecap="round" '
            f'stroke-linejoin="round">{body}</g>')


def word(cx, cy, c):
    return g(c, f"""<path d="M{cx-44} {cy-60} H{cx+16} L{cx+44} {cy-32} V{cy+60} H{cx-44} Z"/>
<path d="M{cx+16} {cy-60} V{cy-32} H{cx+44}"/>
<path d="M{cx-22} {cy-6} H{cx+22}"/><path d="M{cx-22} {cy+16} H{cx+22}"/><path d="M{cx-22} {cy+38} H{cx+6}"/>""")


def grid(cx, cy, c):
    return g(c, f"""<rect x="{cx-60}" y="{cy-50}" width="120" height="100" rx="10"/>
<path d="M{cx-20} {cy-50} V{cy+50}"/><path d="M{cx+20} {cy-50} V{cy+50}"/>
<path d="M{cx-60} {cy-17} H{cx+60}"/><path d="M{cx-60} {cy+17} H{cx+60}"/>""")


def deck(cx, cy, c):
    return g(c, f"""<rect x="{cx-62}" y="{cy-54}" width="124" height="80" rx="10"/>
<path d="M{cx} {cy+26} V{cy+54}"/><path d="M{cx-30} {cy+56} H{cx+30}"/>
<path d="M{cx-40} {cy+8} L{cx-16} {cy-14} L{cx+4} {cy-2} L{cx+38} {cy-32}"/>""")


def page(cx, cy, c, mid):
    # Page outline with a "PDF" label over its lower-left; the outline is cut away behind the label.
    lx, ly, lw, lh = cx - 60, cy + 2, 92, 42
    cut = (f'<mask id="labelcut{mid}" maskUnits="userSpaceOnUse" x="-200" y="-200" width="1000" height="1000">'
           f'<rect x="-200" y="-200" width="1000" height="1000" fill="#fff"/>'
           f'<rect x="{lx-11}" y="{ly-11}" width="{lw+22}" height="{lh+22}" rx="16" fill="#000"/></mask>')
    return cut + g(c, f"""<g mask="url(#labelcut{mid})">
<path d="M{cx-38} {cy-60} H{cx+18} L{cx+46} {cy-32} V{cy+60} H{cx-38} Z"/>
<path d="M{cx+18} {cy-60} V{cy-32} H{cx+46}"/></g>
<rect x="{lx}" y="{ly}" width="{lw}" height="{lh}" rx="10"/>""") + (
        f'<text x="{lx + lw/2}" y="{ly + lh/2 + 10}" text-anchor="middle" '
        f'font-family="Segoe UI, Arial, sans-serif" font-size="28" font-weight="900" '
        f'letter-spacing="1" fill="{c}">PDF</text>')


def art(colors, mid=""):
    w, gr, d, p = colors
    return (word(C1, C1, w) + grid(C2, C1, gr) + deck(C1, C2, d) + page(C2, C2, p, mid))


def svg(viewbox, body):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{viewbox}" width="512" height="512">'
            f'<defs>{FILTERS}</defs>{body}</svg>')


def variant(colors, tint):
    legacy = svg("0 0 512 512", f"""<clipPath id="sq"><rect width="512" height="512" rx="120"/></clipPath>
<g clip-path="url(#sq)">{slate(0, 0, 512, 512)}</g>
<rect x="22" y="22" width="468" height="468" rx="100" fill="none" stroke="#fff" stroke-opacity="0.07" stroke-width="3"/>
<rect x="1.5" y="1.5" width="509" height="509" rx="119" fill="none" stroke="#000" stroke-opacity="0.35" stroke-width="3"/>
{tiles(tint)}{art(colors)}""")
    roundi = svg("0 0 512 512", f"""<clipPath id="ci"><circle cx="256" cy="256" r="256"/></clipPath>
<g clip-path="url(#ci)">{slate(0, 0, 512, 512)}</g>
<circle cx="256" cy="256" r="254.5" fill="none" stroke="#000" stroke-opacity="0.35" stroke-width="3"/>
{tiles(tint)}{art(colors)}""")
    vb = "-128 -128 768 768"   # adaptive: 512 artwork = central 72dp of the 108dp canvas
    return {
        "master/document-slate-icon.svg": legacy,
        "master/document-slate-icon-round.svg": roundi,
        "master/adaptive-background.svg": svg(vb, slate(-128, -128, 768, 768)),
        "master/adaptive-foreground.svg": svg(vb, tiles(tint) + art(colors)),
        "master/adaptive-monochrome.svg": svg(vb, art(["#FFFFFF"] * 4)),
    }


def render_all(jobs):
    """Rasterise every (svg, png_path, px) job in ONE headless Edge page: each SVG is drawn into a
    canvas at its size and read back as PNG data (Edge's own screenshots hang at tiny sizes)."""
    import json, base64, re
    imgs = "".join(
        f'<img id="i{n}" width="{px}" height="{px}" src="data:image/svg+xml;base64,'
        + base64.b64encode(s.replace('width="512" height="512"', f'width="{px}" height="{px}"', 1)
                           .encode()).decode() + '">'
        for n, (s, _, px) in enumerate(jobs))
    page = ("<html><body>" + imgs + "<pre id=o></pre><script>"
            "window.addEventListener('load', () => { const out = [];"
            "document.querySelectorAll('img').forEach(img => { const c = document.createElement('canvas');"
            "c.width = c.height = img.width; c.getContext('2d').drawImage(img, 0, 0, img.width, img.width);"
            "out.push(c.toDataURL('image/png')); });"
            "document.getElementById('o').textContent = out.join(' '); });"
            "</script></body></html>")
    with tempfile.TemporaryDirectory() as td:
        f = pathlib.Path(td, "r.html")
        f.write_text(page, encoding="utf-8")
        r = subprocess.run([EDGE, "--headless=new", "--disable-gpu", f"--user-data-dir={td}\prof",
                            "--virtual-time-budget=20000", "--dump-dom", f.as_uri()],
                           capture_output=True, timeout=300, text=True)
    m = re.search(r'<pre id="o">(.*?)</pre>', r.stdout, re.S)
    urls = m.group(1).split() if m else []
    assert len(urls) == len(jobs) and all(u.startswith("data:image/png") for u in urls), r.stdout[:500]
    for (_, path, _), u in zip(jobs, urls):
        out = pathlib.Path(path); out.parent.mkdir(parents=True, exist_ok=True)
        out.write_bytes(base64.b64decode(u.split(",", 1)[1]))


ADAPTIVE_XML = """<?xml version="1.0" encoding="utf-8"?>
<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
    <background android:drawable="@mipmap/ic_launcher_background" />
    <foreground android:drawable="@mipmap/ic_launcher_foreground" />
    <monochrome android:drawable="@mipmap/ic_launcher_monochrome" />
</adaptive-icon>
"""


def build(folder, files):
    out = HERE / folder
    for p, s in files.items():
        (out / p).parent.mkdir(parents=True, exist_ok=True)
        (out / p).write_text(s, encoding="utf-8")
    legacy, roundi = files["master/document-slate-icon.svg"], files["master/document-slate-icon-round.svg"]
    fg, bg = files["master/adaptive-foreground.svg"], files["master/adaptive-background.svg"]
    mono = files["master/adaptive-monochrome.svg"]
    jobs = []
    for d, k in {"mdpi": 1, "hdpi": 1.5, "xhdpi": 2, "xxhdpi": 3, "xxxhdpi": 4}.items():
        res = out / f"android/res/mipmap-{d}"
        jobs += [(legacy, res / "ic_launcher.png", int(48 * k)),
                 (roundi, res / "ic_launcher_round.png", int(48 * k)),
                 (fg, res / "ic_launcher_foreground.png", int(108 * k)),
                 (bg, res / "ic_launcher_background.png", int(108 * k)),
                 (mono, res / "ic_launcher_monochrome.png", int(108 * k))]
    jobs += [(legacy, out / "png/document-slate-icon-512.png", 512),
             (legacy, out / "png/document-slate-icon-1024.png", 1024)]
    render_all(jobs)
    xd = out / "android/res/mipmap-anydpi-v26"
    xd.mkdir(parents=True, exist_ok=True)
    for n in ("ic_launcher.xml", "ic_launcher_round.xml"):
        (xd / n).write_text(ADAPTIVE_XML, encoding="utf-8")
    print("built", folder)


if __name__ == "__main__":
    build("v2-gray", variant([CHALK] * 4, [None] * 4))
    cols = [APP["word"], APP["grid"], APP["deck"], APP["page"]]
    build("v2-color", variant(cols, cols))
