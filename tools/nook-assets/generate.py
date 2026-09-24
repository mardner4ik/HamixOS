#!/usr/bin/env python3
import os
import struct
import subprocess
import tempfile

import numpy as np
from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(ROOT, "overlay", "usr", "share", "nook")
WALLPAPERS = os.path.join(ROOT, "overlay", "usr", "share", "wallpapers")
FONT_DIR = "/usr/share/fonts/noto"

CHARSET = (
    list(range(0x20, 0x7F))
    + list(range(0xA0, 0x100))
    + list(range(0x400, 0x460))
    + [0x490, 0x491, 0x2013, 0x2014, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2026, 0x20AC, 0x2116,
       0x2190, 0x2191, 0x2192, 0x2193, 0x2713, 0x25B2, 0x25BC, 0x25B6, 0x25C0, 0x2212, 0x2039, 0x203A]
)


def font_coverage(path):
    import subprocess
    out = subprocess.run(["fc-query", "--format=%{charset}", path], capture_output=True, text=True).stdout
    covered = set()
    for part in out.split():
        lo, _, hi = part.partition("-")
        covered.update(range(int(lo, 16), int(hi or lo, 16) + 1))
    return covered


MONO_RANGES = [
    (0x20, 0x7E), (0xA0, 0x17F), (0x370, 0x3FF), (0x400, 0x52F), (0x2000, 0x206F), (0x20A0, 0x20C0),
    (0x2100, 0x215F), (0x2190, 0x21FF), (0x2200, 0x22FF), (0x2300, 0x23FF), (0x2460, 0x24FF), (0x25A0, 0x25FF),
    (0x2600, 0x26FF), (0x2700, 0x27BF),
]


def mono_charset(path):
    covered = font_coverage(path)
    return [cp for lo, hi in MONO_RANGES for cp in range(lo, hi + 1) if cp in covered]


def build_mono_fonts():
    regular = f"{FONT_DIR}/NotoSansMono-Regular.ttf"
    bold = f"{FONT_DIR}/NotoSansMono-Bold.ttf"
    build_font(regular, 14, "mono-14", mono_charset(regular))
    build_font(bold, 14, "mono-bold-14", mono_charset(bold))


def build_font(path, size, name, charset=CHARSET):
    font = ImageFont.truetype(path, size)
    ascent, descent = font.getmetrics()
    glyphs = []
    for cp in charset:
        ch = chr(cp)
        try:
            left, top, right, bottom = font.getbbox(ch, anchor="ls")
        except Exception:
            continue
        advance = int(round(font.getlength(ch)))
        w, h = max(0, right - left), max(0, bottom - top)
        if w == 0 or h == 0:
            glyphs.append((cp, None, 0, 0, advance))
            continue
        img = Image.new("L", (w, h), 0)
        ImageDraw.Draw(img).text((-left, -top), ch, font=font, anchor="ls", fill=255)
        glyphs.append((cp, img, left, -top, advance))
    atlas_w = 512
    x = y = row_h = 0
    placed = []
    for cp, img, bx, by, adv in glyphs:
        if img is None:
            placed.append((cp, 0, 0, 0, 0, 0, 0, adv))
            continue
        if x + img.width + 1 > atlas_w:
            x = 0
            y += row_h + 1
            row_h = 0
        placed.append((cp, x, y, img.width, img.height, bx, by, adv, img))
        x += img.width + 1
        row_h = max(row_h, img.height)
    atlas_h = y + row_h + 1
    atlas = Image.new("L", (atlas_w, atlas_h), 0)
    records = b""
    for entry in placed:
        cp, gx, gy, gw, gh, bx, by, adv = entry[:8]
        if len(entry) == 9:
            atlas.paste(entry[8], (gx, gy))
        records += struct.pack("<IHHBBbbHH", cp, gx, gy, gw, gh, max(-128, min(127, bx)), max(-128, min(127, by)), adv, 0)
    header = b"HFNT" + struct.pack("<HHhhHIHH", 1, size, ascent, descent, ascent + descent + max(2, size // 6), len(placed), atlas_w, atlas_h)
    os.makedirs(os.path.join(OUT, "fonts"), exist_ok=True)
    with open(os.path.join(OUT, "fonts", name + ".hfnt"), "wb") as f:
        f.write(header + records + atlas.tobytes())


def render_svg(svg, size, path):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with tempfile.NamedTemporaryFile("w", suffix=".svg", delete=False) as f:
        f.write(svg)
        name = f.name
    subprocess.run(["rsvg-convert", "-w", str(size), "-h", str(size), "-o", path, name], check=True)
    os.remove(name)
    Image.open(path).convert("RGBA").save(path, optimize=True)


def app_icon(top, bottom, glyph):
    return f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 48 48">
<defs>
<linearGradient id="bg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="{top}"/><stop offset="1" stop-color="{bottom}"/></linearGradient>
<linearGradient id="hl" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fff" stop-opacity=".28"/><stop offset=".5" stop-color="#fff" stop-opacity="0"/></linearGradient>
<filter id="sh" x="-20%" y="-20%" width="140%" height="140%"><feDropShadow dx="0" dy="1" stdDeviation="1" flood-color="#000" flood-opacity=".35"/></filter>
</defs>
<rect x="3" y="3" width="42" height="42" rx="11" fill="url(#bg)" filter="url(#sh)"/>
<rect x="3" y="3" width="42" height="42" rx="11" fill="url(#hl)"/>
<rect x="3.5" y="3.5" width="41" height="41" rx="10.5" fill="none" stroke="#fff" stroke-opacity=".12"/>
<g fill="none" stroke="#fff" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round">{glyph}</g>
</svg>'''


APPS = {
    "files": ("#5aa0ff", "#2f6fe0", '<path d="M12 17.5a3 3 0 0 1 3-3h5.5l3 3h9.5a3 3 0 0 1 3 3v9.5a3 3 0 0 1-3 3H15a3 3 0 0 1-3-3z" fill="#fff" stroke="none" opacity=".96"/><path d="M12 21h24" stroke="#2f6fe0" stroke-opacity=".35" stroke-width="1.5"/>'),
    "terminal": ("#4a5160", "#1f232b", '<path d="M14 18l6 6-6 6"/><path d="M24 31h10"/>'),
    "images": ("#2dd4bf", "#0e8f8f", '<rect x="12" y="14" width="24" height="20" rx="3"/><path d="M13 31l7-7 5 5 3-3 7 7" /><circle cx="29.5" cy="20" r="2.2" fill="#fff" stroke="none"/>'),
    "notes": ("#ffc24b", "#f08c1a", '<path d="M16 12h12l6 6v17a2 2 0 0 1-2 2H16a2 2 0 0 1-2-2V14a2 2 0 0 1 2-2z" fill="#fff" stroke="none"/><path d="M18 22h12M18 27h12M18 32h8" stroke="#f08c1a"/>'),
    "calculator": ("#a78bfa", "#6d44e0", '<rect x="14" y="11" width="20" height="26" rx="3"/><path d="M18 17h12" /><circle cx="19" cy="24" r="1.4" fill="#fff" stroke="none"/><circle cx="24" cy="24" r="1.4" fill="#fff" stroke="none"/><circle cx="29" cy="24" r="1.4" fill="#fff" stroke="none"/><circle cx="19" cy="30" r="1.4" fill="#fff" stroke="none"/><circle cx="24" cy="30" r="1.4" fill="#fff" stroke="none"/><circle cx="29" cy="30" r="1.4" fill="#fff" stroke="none"/>'),
    "monitor": ("#ff7a66", "#e0443a", '<path d="M11 25h6l3-8 5 15 3-9 2 2h7"/>'),
    "settings": ("#98a2b3", "#5b6474", '<circle cx="24" cy="24" r="10" stroke-width="5" stroke-dasharray="3.93 3.93"/><circle cx="24" cy="24" r="6.5" stroke-width="3"/>'),
    "install": ("#6d9bff", "#8b5cf6", '<path d="M24 11v14"/><path d="M18.5 20l5.5 5.5 5.5-5.5"/><rect x="12" y="29" width="24" height="8" rx="2.5"/><circle cx="31" cy="33" r="1.2" fill="#fff" stroke="none"/>'),
    "commands": ("#6e7bff", "#3b44c4", '<path d="M15 17l4 4-4 4"/><path d="M22 25h6"/><path d="M15 31h18"/>'),
    "hello": ("#f472b6", "#db2777", '<path d="M17 25c0-6 3-11 7-11s7 5 7 11"/><path d="M14 30c3 4 17 4 20 0"/>'),
    "folder-home": ("#5aa0ff", "#2f6fe0", '<path d="M14 23l10-9 10 9"/><path d="M17 21v13h14V21"/><path d="M22 34v-6h4v6"/>'),
    "linux-app": ("#f5b83d", "#d9771a", '<path d="M24 12l10 5.5v12L24 35l-10-5.5v-12z"/><path d="M14 17.5l10 5.5 10-5.5"/><path d="M24 23v12"/>'),
    "videos": ("#ff6b8b", "#d9245a", '<rect x="11" y="14" width="26" height="20" rx="4"/><path d="M21.5 19.2v9.6l8-4.8z" fill="#fff" stroke="none"/>'),
}

SPEAKER = '<path d="M2.5 6.2h2.3L8 3.5v9L4.8 9.8H2.5z"/>'
WAVE_1 = '<path d="M10.3 6.3a2.4 2.4 0 0 1 0 3.4"{}/>'
WAVE_2 = '<path d="M12 4.6a4.8 4.8 0 0 1 0 6.8"{}/>'
WAVE_3 = '<path d="M13.7 2.9a7.2 7.2 0 0 1 0 10.2"{}/>'
FADED = ' stroke-opacity=".3"'


def volume_symbol(level):
    waves = [WAVE_1, WAVE_2, WAVE_3]
    return SPEAKER + "".join(w.format("" if i < level else FADED) for i, w in enumerate(waves))

def symbolic(body, stroke=True):
    attrs = 'fill="none" stroke="#fff" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"' if stroke else 'fill="#fff"'
    return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><g {attrs}>{body}</g></svg>'


SYMBOLS = {
    "close": symbolic('<path d="M4 4l8 8M12 4l-8 8"/>'),
    "minimize": symbolic('<path d="M3.5 8.5h9"/>'),
    "maximize": symbolic('<rect x="3.5" y="3.5" width="9" height="9" rx="1.5"/>'),
    "power": symbolic('<path d="M8 2v6"/><path d="M4.6 4.2a5 5 0 1 0 6.8 0"/>'),
    "logout": symbolic('<path d="M6 3H3.5v10H6"/><path d="M10 5l3 3-3 3M13 8H6.5"/>'),
    "reboot": symbolic('<path d="M13 8a5 5 0 1 1-1.5-3.6"/><path d="M12.5 2v3h-3"/>'),
    "user": symbolic('<circle cx="8" cy="5.5" r="2.8"/><path d="M2.8 14c.6-2.8 2.7-4.3 5.2-4.3s4.6 1.5 5.2 4.3"/>'),
    "home": symbolic('<path d="M2.5 7.5L8 3l5.5 4.5"/><path d="M4 6.5V13h8V6.5"/>'),
    "up": symbolic('<path d="M8 13V3M3.5 7.5L8 3l4.5 4.5"/>'),
    "back": symbolic('<path d="M13 8H3M7.5 3.5L3 8l4.5 4.5"/>'),
    "forward": symbolic('<path d="M3 8h10M8.5 3.5L13 8l-4.5 4.5"/>'),
    "refresh": symbolic('<path d="M13 8a5 5 0 1 1-1.5-3.6"/><path d="M12.5 2v3h-3"/>'),
    "folder": symbolic('<path d="M2.5 4.5a1 1 0 0 1 1-1h3l1.5 1.5h4.5a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1h-9a1 1 0 0 1-1-1z"/>'),
    "file": symbolic('<path d="M4 2.5h5l3 3v8H4z"/><path d="M9 2.5v3h3"/>'),
    "search": symbolic('<circle cx="7" cy="7" r="4"/><path d="M10 10l3.5 3.5"/>'),
    "check": symbolic('<path d="M3 8.5l3.2 3L13 4.5"/>'),
    "disk": symbolic('<rect x="2.5" y="4" width="11" height="8" rx="1.5"/><path d="M5 9.5h3"/><circle cx="11" cy="9.5" r=".6" fill="#fff"/>'),
    "plus": symbolic('<path d="M8 3v10M3 8h10"/>'),
    "minus": symbolic('<path d="M3 8h10"/>'),
    "apps": symbolic('<rect x="2.5" y="2.5" width="4" height="4" rx="1"/><rect x="9.5" y="2.5" width="4" height="4" rx="1"/><rect x="2.5" y="9.5" width="4" height="4" rx="1"/><rect x="9.5" y="9.5" width="4" height="4" rx="1"/>'),
    "trash": symbolic('<path d="M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.7 9h5.6l.7-9"/>'),
    "zoom-in": symbolic('<circle cx="7" cy="7" r="4"/><path d="M10 10l3.5 3.5M5 7h4M7 5v4"/>'),
    "zoom-out": symbolic('<circle cx="7" cy="7" r="4"/><path d="M10 10l3.5 3.5M5 7h4"/>'),
    "fit": symbolic('<path d="M2.5 6V2.5H6M10 2.5h3.5V6M13.5 10v3.5H10M6 13.5H2.5V10"/>'),
    "image": symbolic('<rect x="2.5" y="3" width="11" height="10" rx="1.5"/><path d="M3 11.5l3.5-3.5 2.5 2.5 1.5-1.5 2.5 2.5"/>'),
    "save": symbolic('<path d="M3 2.5h8l2 2v9H3z"/><path d="M5.5 2.5v3h5v-3M5.5 13.5v-4h5v4"/>'),
    "open": symbolic('<path d="M2.5 12.5v-8a1 1 0 0 1 1-1h3l1.5 1.5h4a1 1 0 0 1 1 1v1"/><path d="M2.5 12.5l2-5h10l-2 5z"/>'),
    "new": symbolic('<path d="M4 2.5h5l3 3v8H4z"/><path d="M8 7v4M6 9h4"/>'),
    "lock": symbolic('<rect x="3.5" y="7" width="9" height="6.5" rx="1.5"/><path d="M5.5 7V5a2.5 2.5 0 0 1 5 0v2"/>'),
    "info": symbolic('<circle cx="8" cy="8" r="5.5"/><path d="M8 7.5v3.5M8 5v.2"/>'),
    "wallpaper": symbolic('<rect x="2.5" y="3" width="11" height="10" rx="1.5"/><path d="M3 11.5l3.5-3.5 2.5 2.5 1.5-1.5 2.5 2.5"/><circle cx="10.5" cy="6" r="1"/>'),
    "session": symbolic('<rect x="2.5" y="3" width="11" height="8" rx="1.5"/><path d="M6 13.5h4M8 11v2.5"/>'),
    "warning": symbolic('<path d="M8 2.5l6 11H2z"/><path d="M8 7v3M8 12v.2"/>'),
    "bell": symbolic('<path d="M4 11.5V7a4 4 0 0 1 8 0v4.5l1 1H3z"/><path d="M6.8 14h2.4"/>'),
    "run": symbolic('<path d="M5 3.5v9l7-4.5z"/>'),
    "copy": symbolic('<rect x="5.5" y="5.5" width="8" height="8" rx="1.5"/><path d="M3 10.5V3.5a1 1 0 0 1 1-1h6.5"/>'),
    "paste": symbolic('<rect x="3.5" y="3.5" width="9" height="10" rx="1.5"/><path d="M6 3.5V2.5h4v1"/>'),
    "chevron-right": symbolic('<path d="M6 3.5L10.5 8 6 12.5"/>'),
    "chevron-down": symbolic('<path d="M3.5 6L8 10.5 12.5 6"/>'),
    "chevron-left": symbolic('<path d="M10 3.5L5.5 8 10 12.5"/>'),
    "restore": symbolic('<rect x="3.5" y="5.5" width="7" height="7" rx="1.2"/><path d="M6 5.5V4a.5.5 0 0 1 .5-.5h5.5a.5.5 0 0 1 .5.5v5.5a.5.5 0 0 1-.5.5h-1.5"/>'),
    "pin": symbolic('<path d="M6 2.5h4l-.5 4 2.5 2.5H4l2.5-2.5z"/><path d="M8 9v4.5"/>'),
    "unpin": symbolic('<path d="M6 2.5h4l-.5 4 2.5 2.5H4l2.5-2.5z"/><path d="M8 9v4.5M2.5 2.5l11 11"/>'),
    "display": symbolic('<rect x="2" y="3" width="12" height="8" rx="1.3"/><path d="M6 13.5h4M8 11v2.5"/>'),
    "network": symbolic('<rect x="2.5" y="9.5" width="11" height="4" rx="1"/><path d="M5 11.5h.2M8 9.5V3.5M5.5 5.5L8 3l2.5 2.5"/>'),
    "ethernet": symbolic('<path d="M3.5 5.5h9v6h-2v1.5h-5V11.5h-2z"/><path d="M6 8v1.5M8 8v1.5M10 8v1.5"/>'),
    "wifi-0": symbolic('<path d="M1.8 6.2a9 9 0 0 1 12.4 0" stroke-opacity=".3"/><path d="M3.9 8.5a6 6 0 0 1 8.2 0" stroke-opacity=".3"/><path d="M6 10.8a3 3 0 0 1 4 0" stroke-opacity=".3"/><circle cx="8" cy="12.8" r="1" fill="#fff" stroke="none"/>'),
    "wifi-1": symbolic('<path d="M1.8 6.2a9 9 0 0 1 12.4 0" stroke-opacity=".3"/><path d="M3.9 8.5a6 6 0 0 1 8.2 0" stroke-opacity=".3"/><path d="M6 10.8a3 3 0 0 1 4 0"/><circle cx="8" cy="12.8" r="1" fill="#fff" stroke="none"/>'),
    "wifi-2": symbolic('<path d="M1.8 6.2a9 9 0 0 1 12.4 0" stroke-opacity=".3"/><path d="M3.9 8.5a6 6 0 0 1 8.2 0"/><path d="M6 10.8a3 3 0 0 1 4 0"/><circle cx="8" cy="12.8" r="1" fill="#fff" stroke="none"/>'),
    "wifi-3": symbolic('<path d="M1.8 6.2a9 9 0 0 1 12.4 0"/><path d="M3.9 8.5a6 6 0 0 1 8.2 0"/><path d="M6 10.8a3 3 0 0 1 4 0"/><circle cx="8" cy="12.8" r="1" fill="#fff" stroke="none"/>'),
    "wifi-off": symbolic('<path d="M1.8 6.2a9 9 0 0 1 12.4 0" stroke-opacity=".3"/><path d="M3.9 8.5a6 6 0 0 1 8.2 0" stroke-opacity=".3"/><path d="M6 10.8a3 3 0 0 1 4 0" stroke-opacity=".3"/><path d="M2.5 2.5l11 11"/>'),
    "network-off": symbolic('<rect x="2.5" y="9.5" width="11" height="4" rx="1" stroke-opacity=".45"/><path d="M8 9.5V3.5" stroke-opacity=".45"/><path d="M2.5 2.5l11 11"/>'),
    "cpu": symbolic('<rect x="4" y="4" width="8" height="8" rx="1.2"/><path d="M6.5 6.5h3v3h-3zM6 2v2M10 2v2M6 12v2M10 12v2M2 6h2M2 10h2M12 6h2M12 10h2"/>'),
    "system": symbolic('<path d="M8 2.5l5 2.5v3.5c0 3-2.2 4.8-5 5.5-2.8-.7-5-2.5-5-5.5V5z"/><path d="M5.8 8.2l1.6 1.6 3-3"/>'),
    "volume-1": symbolic(volume_symbol(1)),
    "volume-2": symbolic(volume_symbol(2)),
    "volume-3": symbolic(volume_symbol(3)),
    "volume-mute": symbolic(SPEAKER + '<path d="M10.5 6l3.5 4M14 6l-3.5 4"/>'),
    "volume-off": symbolic(SPEAKER + '<path d="M2 2l12 12"/>'),
    "play": symbolic('<path d="M5 3.2v9.6l7.5-4.8z"/>', stroke=False),
    "pause": symbolic('<rect x="4" y="3" width="2.8" height="10" rx="1"/><rect x="9.2" y="3" width="2.8" height="10" rx="1"/>', stroke=False),
    "seek-back": symbolic('<path d="M7.6 4.2v7.6L2.5 8zM13.5 4.2v7.6L8.4 8z"/>', stroke=False),
    "seek-forward": symbolic('<path d="M8.4 4.2v7.6L13.5 8zM2.5 4.2v7.6L7.6 8z"/>', stroke=False),
    "fullscreen": symbolic('<path d="M2.5 6V2.5H6M10 2.5h3.5V6M13.5 10v3.5H10M6 13.5H2.5V10"/>'),
    "video": symbolic('<rect x="2.5" y="3.5" width="11" height="9" rx="1.5"/><path d="M6.8 6.1v3.8L10 8z" fill="#fff"/>'),
    "sound": symbolic(volume_symbol(3)),
}


def file_icon(color, fold, glyph):
    return f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32">
<path d="M8 3h11l7 7v17a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z" fill="#eef1f6"/>
<path d="M19 3v5a2 2 0 0 0 2 2h5" fill="{fold}"/>
<path d="M8 3h11l7 7v17a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z" fill="none" stroke="#000" stroke-opacity=".12"/>
<g fill="none" stroke="{color}" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">{glyph}</g>
</svg>'''


FILES = {
    "folder": '''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32"><defs><linearGradient id="f" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#6aaeff"/><stop offset="1" stop-color="#3b7cf0"/></linearGradient></defs>
<path d="M3 8a2 2 0 0 1 2-2h7l3 3h12a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" fill="#2f63c9"/>
<path d="M3 12a2 2 0 0 1 2-2h22a2 2 0 0 1 2 2v13a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" fill="url(#f)"/></svg>''',
    "file": file_icon("#8a93a3", "#c9d0db", ""),
    "text": file_icon("#6b7485", "#c9d0db", '<path d="M10 15h12M10 19h12M10 23h8"/>'),
    "image": file_icon("#0e9f8f", "#9fe7dc", '<path d="M9.5 24l4.5-4.5 3 3 2-2 4 3.5"/><circle cx="19.5" cy="15.5" r="1.6"/>'),
    "exec": file_icon("#5b6474", "#c9d0db", '<path d="M10 16l3 3-3 3"/><path d="M15.5 22h6"/>'),
    "script": file_icon("#2f9e44", "#b2f2bb", '<path d="M10 16l3 3-3 3"/><path d="M15.5 22h6"/>'),
    "device": file_icon("#d97706", "#fde68a", '<rect x="10" y="15" width="12" height="8" rx="1.5"/>'),
    "video": file_icon("#d9245a", "#fbc4d4", '<rect x="9.5" y="14.5" width="13" height="10" rx="2"/><path d="M14.6 17.2v4.6l3.8-2.3z" fill="#d9245a" stroke="none"/>'),
}

CURSOR_ARROW = '''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32">
<defs><filter id="s" x="-50%" y="-50%" width="200%" height="200%"><feDropShadow dx="0.8" dy="1.4" stdDeviation="1.1" flood-color="#000" flood-opacity=".45"/></filter></defs>
<path d="M5 3.5v19.2l4.6-4.3 3.1 7.1 3.3-1.4-3-7h6.4z" fill="#0b0c0f" stroke="#fff" stroke-width="1.35" stroke-linejoin="round" filter="url(#s)"/>
</svg>'''

CURSOR_TEXT = '''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32">
<path d="M12 5h3l1 1 1-1h3M16 6v20M12 27h3l1-1 1 1h3" fill="none" stroke="#fff" stroke-width="3.6" stroke-linecap="round"/>
<path d="M12 5h3l1 1 1-1h3M16 6v20M12 27h3l1-1 1 1h3" fill="none" stroke="#0b0c0f" stroke-width="1.6" stroke-linecap="round"/>
</svg>'''

CURSOR_HAND = '''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32">
<defs><filter id="s" x="-50%" y="-50%" width="200%" height="200%"><feDropShadow dx="0.8" dy="1.4" stdDeviation="1.1" flood-color="#000" flood-opacity=".45"/></filter></defs>
<path d="M11 4.5a1.8 1.8 0 0 1 3.6 0V13l1.4-.3a1.7 1.7 0 0 1 3.3.4l1.4-.2a1.7 1.7 0 0 1 3.1.8l1-.1a1.7 1.7 0 0 1 2.2 1.6v5.3c0 4.1-2.6 7-6.8 7h-2.4c-2.3 0-3.8-.9-5.1-2.6l-4.4-6.1a1.8 1.8 0 0 1 2.7-2.3L11 17.6z" fill="#0b0c0f" stroke="#fff" stroke-width="1.35" stroke-linejoin="round" filter="url(#s)"/>
</svg>'''

CURSOR_MOVE = '''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32">
<path d="M16 3l4 4h-2.6v7.6H25V12l4 4-4 4v-2.6h-7.6V25H20l-4 4-4-4h2.6v-7.6H7V20l-4-4 4-4v2.6h7.6V7H12z" fill="#0b0c0f" stroke="#fff" stroke-width="1.3" stroke-linejoin="round"/>
</svg>'''


def resize_cursor(rotation):
    return f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32">
<g transform="rotate({rotation} 16 16)"><path d="M16 4l5.5 6h-3.6v12h3.6L16 28l-5.5-6h3.6V10h-3.6z" fill="#0b0c0f" stroke="#fff" stroke-width="1.35" stroke-linejoin="round"/></g>
</svg>'''


def wallpaper(name, base_top, base_bottom, blobs, size=(1920, 1080)):
    w, h = size
    small = (w // 4, h // 4)
    img = Image.new("RGB", small)
    top = np.array(base_top, dtype=np.float32)
    bottom = np.array(base_bottom, dtype=np.float32)
    t = np.linspace(0, 1, small[1], dtype=np.float32)[:, None, None]
    arr = top * (1 - t) + bottom * t
    arr = np.repeat(arr, small[0], axis=1)
    img = Image.fromarray(arr.clip(0, 255).astype("uint8"))
    layer = Image.new("RGBA", small, (0, 0, 0, 0))
    draw = ImageDraw.Draw(layer)
    for (cx, cy, rx, ry, color, alpha) in blobs:
        x, y = cx * small[0], cy * small[1]
        draw.ellipse([x - rx * small[0], y - ry * small[1], x + rx * small[0], y + ry * small[1]], fill=color + (alpha,))
    layer = layer.filter(ImageFilter.GaussianBlur(small[0] / 9))
    img = Image.alpha_composite(img.convert("RGBA"), layer).convert("RGB")
    img = img.resize(size, Image.BICUBIC).filter(ImageFilter.GaussianBlur(2))
    arr = np.asarray(img).astype(np.float32)
    yy, xx = np.mgrid[0:h, 0:w]
    vignette = 1 - 0.28 * (((xx - w / 2) / (w / 1.3)) ** 2 + ((yy - h / 2) / (h / 1.1)) ** 2)
    arr = arr * vignette[:, :, None]
    bayer = np.array([[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]], dtype=np.float32) / 16 - 0.5
    arr = arr + np.tile(bayer, (h // 4 + 1, w // 4 + 1))[:h, :w, None]
    os.makedirs(WALLPAPERS, exist_ok=True)
    Image.fromarray(arr.clip(0, 255).astype("uint8")).save(os.path.join(WALLPAPERS, name + ".png"), optimize=True)

ICON_THEME = os.path.join(ROOT, "overlay", "usr", "share", "icons", "Nook Icons")
ICON_THEME_SIZES = [24, 48, 128]


def page(color, marks):
    return (f'<path d="M16 11h11l7 7v17a2 2 0 0 1-2 2H16a2 2 0 0 1-2-2V13a2 2 0 0 1 2-2z" fill="#fff" stroke="none"/>'
            f'<path d="M27 11.5v5.5a1 1 0 0 0 1 1h5.5" stroke="{color}" stroke-opacity=".45" stroke-width="1.6"/>'
            f'<g stroke="{color}" stroke-width="2">{marks}</g>')


THEME_APPS = {
    "libreoffice-startcenter": ("#6ea8ff", "#2b64c8", page("#2b64c8", '<path d="M18.5 22h11M18.5 26.5h11M18.5 31h7"/>'), []),
    "libreoffice-writer": ("#5aa0ff", "#1f5fd0", page("#1f5fd0", '<path d="M18.5 21h11M18.5 25h11M18.5 29h11M18.5 33h6"/>'), ["writer"]),
    "libreoffice-calc": ("#4ade80", "#15924a", page("#15924a", '<rect x="18" y="20" width="12" height="13" rx="1" stroke-width="1.6"/><path d="M18 24.5h12M18 28.8h12M22 20v13M26 20v13" stroke-width="1.4"/>'), ["calc"]),
    "libreoffice-impress": ("#ff9f5a", "#e0621a", page("#e0621a", '<rect x="18" y="20" width="12" height="9" rx="1" stroke-width="1.6"/><path d="M24 29v4M21 33h6"/>'), ["impress"]),
    "libreoffice-draw": ("#ffd24d", "#e0a412", page("#c98a06", '<circle cx="21.5" cy="23.5" r="3" stroke-width="1.8"/><path d="M24 33l3.5-6.5 3.5 6.5z" stroke-width="1.6"/>'), ["draw"]),
    "libreoffice-base": ("#c084fc", "#8b3fd9", page("#8b3fd9", '<ellipse cx="24" cy="21.5" rx="5.5" ry="2"/><path d="M18.5 21.5v10c0 1.1 2.5 2 5.5 2s5.5-.9 5.5-2v-10M18.5 26.5c0 1.1 2.5 2 5.5 2s5.5-.9 5.5-2" stroke-width="1.8"/>'), ["base"]),
    "libreoffice-math": ("#9ca3af", "#4b5563", page("#4b5563", '<path d="M19 27h2.5l2 5 3.5-12h3"/>'), ["math"]),
    "org.telegram.desktop": ("#5ec8f5", "#1f8ed8", '<path d="M11.5 23.3l21.8-8.6c1-.4 2 .5 1.7 1.6l-3.6 16.9c-.2 1-1.4 1.5-2.3.9l-5.6-4.1-2.9 2.8c-.4.4-1.1.2-1.2-.4l-.8-5.3-7.1-2.3c-1-.3-1-1.7 0-2.1z" fill="#fff" stroke="none"/><path d="M20.8 26.3l8.5-6.6" stroke="#1f8ed8" stroke-width="1.6"/>', ["telegram", "telegram-desktop", "telegramdesktop"]),
    "kate": ("#7c8cff", "#3949c8", '<path d="M15 13v22"/><path d="M29 13l-10.5 11L29 35"/><path d="M27.5 27.5l6-6" stroke-width="2.2"/><path d="M31.5 19.5l3 3"/>', ["org.kde.kate", "kwrite", "org.kde.kwrite"]),
    "chromium": ("#6aa7ff", "#2459c8", '<circle cx="24" cy="24" r="11.5"/><circle cx="24" cy="24" r="4.5" fill="#fff" stroke="none"/><path d="M24.00 19.50L34.58 19.50 M27.90 26.25L22.61 35.42 M20.10 26.25L14.81 17.08" stroke-width="2.2"/>', ["chromium-browser", "org.chromium.Chromium", "google-chrome"]),
    "firefox": ("#ffab40", "#d9368b", '<circle cx="24" cy="25" r="10.5"/><path d="M15.5 18.5c1.5-5 6.5-7.5 11-6.5-2.2 1-3.2 2.6-3.3 4.3 4.3-.5 8.7 2.4 9.4 7.2"/><path d="M19 26.5c1.5 3 5.8 4 8.6 1.6"/>', ["firefox-esr", "org.mozilla.firefox"]),
    "thunderbird": ("#5aa0ff", "#1d4fb8", '<rect x="12" y="16" width="24" height="17" rx="3"/><path d="M13 18l11 8 11-8"/>', ["org.mozilla.Thunderbird", "thunderbird-esr"]),
    "gimp": ("#9aa06a", "#5a5f33", '<path d="M29 13l5 5-11.5 11.5-5-5z" fill="#fff" stroke="none"/><path d="M17.5 24.5c-3 .5-4.5 3-4.5 6 0 1.8-.8 3-2 3.5 5 2 10.5-.2 11.5-5"/>', ["org.gimp.GIMP"]),
    "inkscape": ("#4b5563", "#111827", '<path d="M24 11l-9 12h5l-6 8h8v6h4v-6h8l-6-8h5z" fill="#fff" stroke="none"/>', ["org.inkscape.Inkscape"]),
    "krita": ("#f472b6", "#9d2c9b", '<path d="M32 12l4 4-12 12-4-4z" fill="#fff" stroke="none"/><path d="M19.5 26.5c-3 0-5.5 2-5.5 5.5 0 1.5-.7 2.5-2 3 5.5 1.5 10-.5 10.5-5.5"/>', ["org.kde.krita"]),
    "blender": ("#ffa94d", "#e0621a", '<circle cx="27" cy="26.5" r="7"/><circle cx="27" cy="26.5" r="2.5" fill="#fff" stroke="none"/><path d="M11 22.5h10M15 17h9.5l-4 4"/>', ["org.blender.Blender"]),
    "vlc": ("#ffb347", "#f07b12", '<path d="M20 16h8l5 18H15z" fill="#fff" stroke="none"/><path d="M18.5 22.5h11M16.8 28.5h14.4" stroke="#f07b12" stroke-width="2.2"/><path d="M12 35h24"/>', ["vlc", "org.videolan.VLC"]),
    "mpv": ("#8b5cf6", "#4c1d95", '<circle cx="24" cy="24" r="11"/><path d="M21 18.5v11l9-5.5z" fill="#fff" stroke="none"/>', ["io.mpv.Mpv"]),
    "audacity": ("#3b82f6", "#1e3a8a", '<path d="M12 24h2M16 19v10M20 14v20M24 18v12M28 12v24M32 17v14M36 22v4"/>', ["org.audacityteam.Audacity"]),
    "obs": ("#475569", "#0f172a", '<circle cx="24" cy="24" r="11"/><circle cx="24" cy="17.5" r="3" fill="#fff" stroke="none"/><circle cx="18.3" cy="27.3" r="3" fill="#fff" stroke="none"/><circle cx="29.7" cy="27.3" r="3" fill="#fff" stroke="none"/>', ["com.obsproject.Studio"]),
    "code": ("#38bdf8", "#0369a1", '<path d="M19 16l-8 8 8 8"/><path d="M29 16l8 8-8 8"/><path d="M26 13l-4 22"/>', ["vscode", "visual-studio-code", "code-oss", "com.visualstudio.code", "vscodium"]),
    "geany": ("#fbbf24", "#b45309", '<path d="M14 22h17v5a8 8 0 0 1-8 8h-1a8 8 0 0 1-8-8z" fill="#fff" stroke="none"/><path d="M31 24h2a3 3 0 0 1 0 6h-2.5"/><path d="M20 13c-1 2 1 3 0 5M25 13c-1 2 1 3 0 5"/>', ["org.geany.Geany"]),
    "transmission": ("#ef4444", "#991b1b", '<path d="M24 12v16"/><path d="M17 22l7 7 7-7"/><path d="M14 35h20"/>', ["transmission-gtk", "transmission-qt", "com.transmissionbt.transmission"]),
    "evince": ("#f87171", "#b91c1c", '<path d="M15 11h11l6 6v12" stroke-width="2.4"/><path d="M15 11v24h8"/><circle cx="28.5" cy="30.5" r="4.5"/><path d="M32 34l3.5 3.5"/>', ["org.gnome.Evince", "zathura", "org.pwmt.zathura", "atril", "okular", "org.kde.okular"]),
    "org.xfce.thunar": ("#60a5fa", "#1d4ed8", '<path d="M12 17.5a3 3 0 0 1 3-3h5.5l3 3h9.5a3 3 0 0 1 3 3v9.5a3 3 0 0 1-3 3H15a3 3 0 0 1-3-3z" fill="#fff" stroke="none"/><path d="M26 20.5l-3.5 5.5h4l-3 5" stroke="#1d4ed8" stroke-width="1.8"/>', ["thunar", "Thunar", "system-file-manager", "org.gnome.Nautilus", "pcmanfm", "nemo", "org.kde.dolphin"]),
    "org.xfce.mousepad": ("#fde047", "#ca8a04", '<path d="M16 12h12l6 6v17a2 2 0 0 1-2 2H16a2 2 0 0 1-2-2V14a2 2 0 0 1 2-2z" fill="#fff" stroke="none"/><path d="M18 22h12M18 27h12M18 32h8" stroke="#ca8a04"/>', ["mousepad", "accessories-text-editor", "org.gnome.TextEditor", "gedit", "org.gnome.gedit", "leafpad", "featherpad"]),
    "galculator": ("#a78bfa", "#6d28d9", '<rect x="14" y="11" width="20" height="26" rx="3"/><path d="M18 17h12"/><path d="M19 24h4M21 22v4M26 24h4M19 31h4M26 30h4M26 32h4"/>', ["accessories-calculator", "org.gnome.Calculator", "gnome-calculator", "qalculate-gtk", "kcalc", "org.kde.kcalc"]),
    "utilities-terminal": ("#4a5160", "#1f232b", '<path d="M14 18l6 6-6 6"/><path d="M24 31h10"/>', ["foot", "footclient", "foot-server", "xterm", "uxterm", "org.xfce.terminal", "xfce4-terminal", "org.gnome.Terminal", "gnome-terminal", "org.kde.konsole", "konsole", "alacritty", "Alacritty", "kitty", "st"]),
    "htop": ("#34d399", "#047857", '<rect x="12" y="13" width="24" height="22" rx="3"/><path d="M16 29v-4M20 29v-8M24 29v-6M28 29v-10M32 29v-3"/>', ["btop", "bashtop", "org.gnome.SystemMonitor", "gnome-system-monitor", "utilities-system-monitor", "xfce4-taskmanager", "org.xfce.taskmanager"]),
    "dillo": ("#60a5fa", "#0e7490", '<circle cx="24" cy="24" r="11"/><path d="M13 24h22M24 13c-4 4-4 18 0 22M24 13c4 4 4 18 0 22"/>', ["web-browser", "netsurf", "netsurf-gtk", "midori", "epiphany", "org.gnome.Epiphany", "falkon", "org.kde.falkon", "qutebrowser"]),
    "mc": ("#38bdf8", "#1e40af", '<rect x="12" y="13" width="11" height="22" rx="2"/><rect x="25" y="13" width="11" height="22" rx="2"/><path d="M15 18h5M28 18h5"/>', ["midnight-commander"]),
    "vim": ("#4ade80", "#166534", '<path d="M14 14h7l3 12 6-12h4l-9 20h-2z" fill="#fff" stroke="none"/>', ["gvim", "nvim", "neovim", "org.vim.Vim"]),
    "steam": ("#475569", "#0f172a", '<circle cx="29" cy="20" r="5"/><circle cx="29" cy="20" r="2" fill="#fff" stroke="none"/><path d="M12 26.5l8 3.5"/><circle cx="21" cy="30.5" r="3"/><path d="M24 29l3-4.5"/>', ["com.valvesoftware.Steam"]),
    "discord": ("#8b93ff", "#4752c4", '<path d="M16 16.5c5-2 11-2 16 0 2 3.5 3.5 8 3 13-2 1.5-4 2.5-6 3l-1.5-2.5M16 16.5c-2 3.5-3.5 8-3 13 2 1.5 4 2.5 6 3l1.5-2.5M17 29c4.5 2 9.5 2 14 0"/><circle cx="20.5" cy="24" r="2" fill="#fff" stroke="none"/><circle cx="27.5" cy="24" r="2" fill="#fff" stroke="none"/>', ["com.discordapp.Discord", "webcord", "vesktop"]),
    "spotify": ("#4ade80", "#15803d", '<circle cx="24" cy="24" r="11"/><path d="M17 20.5c5-1.5 10-1 14.5 1.5M18 25c4-1 8-.6 11.5 1.2M19 29.2c3-.7 6-.4 8.5.8"/>', ["com.spotify.Client", "spotify-client"]),
    "libreoffice": ("#6ea8ff", "#2b64c8", page("#2b64c8", '<path d="M18.5 22h11M18.5 26.5h11M18.5 31h7"/>'), []),
}


def build_icon_theme():
    dirs = []
    for size in ICON_THEME_SIZES:
        dirs.append(f"{size}x{size}/apps")
    for name, (top, bottom, glyph, aliases) in THEME_APPS.items():
        svg = app_icon(top, bottom, glyph)
        for size in ICON_THEME_SIZES:
            for target in [name] + aliases:
                render_svg(svg, size, os.path.join(ICON_THEME, f"{size}x{size}", "apps", target + ".png"))
    with open(os.path.join(ICON_THEME, "index.theme"), "w") as f:
        f.write("[Icon Theme]\nName=Nook Icons\nComment=HamixOS icons for Nook and popular Linux applications\nInherits=Adwaita,hicolor\nExample=utilities-terminal\n")
        f.write("Directories=" + ",".join(dirs) + "\n\n")
        for size in ICON_THEME_SIZES:
            f.write(f"[{size}x{size}/apps]\nSize={size}\nContext=Applications\nType=Fixed\n\n")



BRAND_TOP = (94, 234, 212)
BRAND_MID = (91, 140, 255)
BRAND_END = (139, 92, 246)


def brand_mask(name):
    image = Image.open(os.path.join(os.path.dirname(os.path.abspath(__file__)), name)).convert("RGBA")
    alpha = image.getchannel("A")
    return image, alpha.crop(alpha.getbbox()), alpha.getbbox()


def fit_mask(mask, size, margin):
    w, h = mask.size
    scale = (size - 2 * margin) / max(w, h)
    small = mask.resize((max(1, round(w * scale)), max(1, round(h * scale))), Image.LANCZOS)
    out = Image.new("L", (size, size), 0)
    out.paste(small, ((size - small.width) // 2, (size - small.height) // 2))
    return out


def gradient(size):
    arr = np.zeros((size, size, 3), dtype=np.float32)
    t = np.add.outer(np.arange(size), np.arange(size)) / max(1, 2 * size - 2)
    for i in range(3):
        first = BRAND_TOP[i] + (BRAND_MID[i] - BRAND_TOP[i]) * np.clip(t * 2, 0, 1)
        second = BRAND_MID[i] + (BRAND_END[i] - BRAND_MID[i]) * np.clip(t * 2 - 1, 0, 1)
        arr[..., i] = np.where(t < 0.5, first, second)
    return Image.fromarray(arr.astype("uint8"), "RGB")


def colored(mask, color):
    layer = Image.new("RGBA", mask.size, color + (255,))
    layer.putalpha(mask)
    return layer


def brand_icons():
    _, mark, _ = brand_mask("hamix-mark.png")
    for size, name, margin in [(64, "logo", 4), (128, "logo-128", 8)]:
        mask = fit_mask(mark, size, margin)
        icon = gradient(size).convert("RGBA")
        icon.putalpha(mask)
        icon.save(os.path.join(OUT, "icons", name + ".png"), optimize=True)
    colored(fit_mask(mark, 20, 0), (255, 255, 255)).save(os.path.join(OUT, "icons", "logo-20.png"), optimize=True)
    for size, name, margin in [(48, "hamix", 3), (24, "hamix-24", 1)]:
        icon = gradient(size).convert("RGBA")
        icon.putalpha(fit_mask(mark, size, margin))
        icon.save(os.path.join(OUT, "icons", "apps", name + ".png"), optimize=True)
    image, _, _ = brand_mask("hamix-wordmark.png")
    rgb = np.asarray(image.convert("RGB")).astype(np.float32).mean(axis=2)
    alpha = np.asarray(image.getchannel("A")).astype(np.float32)
    bbox = image.getchannel("A").getbbox()
    mark_part = Image.fromarray((alpha * (rgb < 128)).astype("uint8")).crop(bbox)
    text_part = Image.fromarray((alpha * (rgb >= 128)).astype("uint8")).crop(bbox)
    width = 240
    height = round(mark_part.height * width / mark_part.width)
    mark_small = mark_part.resize((width, height), Image.LANCZOS)
    text_small = text_part.resize((width, height), Image.LANCZOS)
    for name, text_color in [("about-logo-dark", (236, 239, 244)), ("about-logo-light", (29, 33, 41))]:
        out = gradient(max(width, height)).resize((width, height)).convert("RGBA")
        out.putalpha(mark_small)
        out.alpha_composite(colored(text_small, text_color))
        out.save(os.path.join(OUT, "icons", name + ".png"), optimize=True)


def main():
    import sys
    icons_only = "--icons" in sys.argv
    if "--theme" in sys.argv:
        return build_icon_theme()
    if icons_only:
        icons()
        return build_icon_theme()
    if "--mono" in sys.argv:
        return build_mono_fonts()
    build_font(f"{FONT_DIR}/NotoSans-Regular.ttf", 13, "sans-13")
    build_font(f"{FONT_DIR}/NotoSans-Regular.ttf", 11, "sans-11")
    build_font(f"{FONT_DIR}/NotoSans-Medium.ttf", 13, "sans-medium-13")
    build_font(f"{FONT_DIR}/NotoSans-Medium.ttf", 18, "sans-medium-18")
    build_font(f"{FONT_DIR}/NotoSans-Light.ttf", 30, "sans-light-30")
    build_mono_fonts()

    icons()
    build_icon_theme()
    return wallpapers()


def icons():
    for name, (top, bottom, glyph) in APPS.items():
        svg = app_icon(top, bottom, glyph)
        render_svg(svg, 48, os.path.join(OUT, "icons", "apps", name + ".png"))
        render_svg(svg, 24, os.path.join(OUT, "icons", "apps", name + "-24.png"))
    brand_icons()
    for name, svg in SYMBOLS.items():
        render_svg(svg, 16, os.path.join(OUT, "icons", "ui", name + ".png"))
    for name, svg in FILES.items():
        render_svg(svg, 32, os.path.join(OUT, "icons", "files", name + ".png"))
        render_svg(svg, 64, os.path.join(OUT, "icons", "files", name + "-64.png"))
    render_svg(CURSOR_ARROW, 32, os.path.join(OUT, "cursors", "arrow.png"))
    render_svg(CURSOR_TEXT, 32, os.path.join(OUT, "cursors", "text.png"))
    render_svg(CURSOR_HAND, 32, os.path.join(OUT, "cursors", "hand.png"))
    render_svg(CURSOR_MOVE, 32, os.path.join(OUT, "cursors", "move.png"))
    for name, rotation in [("resize-ns", 0), ("resize-ew", 90), ("resize-nwse", -45), ("resize-nesw", 45)]:
        render_svg(resize_cursor(rotation), 32, os.path.join(OUT, "cursors", name + ".png"))
    with open(os.path.join(OUT, "cursors", "hotspots"), "w") as f:
        f.write("arrow 5 3\ntext 16 16\nhand 12 3\nmove 16 16\nresize-ns 16 16\nresize-ew 16 16\nresize-nwse 16 16\nresize-nesw 16 16\n")


def wallpapers():

    wallpaper("dusk", (18, 24, 44), (10, 12, 22), [
        (0.25, 0.30, 0.35, 0.45, (72, 104, 230), 150),
        (0.75, 0.65, 0.40, 0.50, (124, 77, 235), 120),
        (0.55, 0.15, 0.25, 0.25, (46, 180, 200), 70),
        (0.15, 0.85, 0.30, 0.30, (230, 90, 140), 60),
    ])
    wallpaper("graphite", (44, 48, 56), (20, 22, 27), [
        (0.30, 0.35, 0.45, 0.40, (90, 100, 120), 110),
        (0.80, 0.75, 0.35, 0.40, (60, 70, 90), 120),
    ])
    wallpaper("aurora", (8, 22, 30), (6, 10, 18), [
        (0.30, 0.25, 0.45, 0.25, (20, 190, 140), 120),
        (0.70, 0.40, 0.40, 0.25, (40, 120, 220), 110),
        (0.50, 0.85, 0.50, 0.25, (120, 60, 200), 80),
    ])


if __name__ == "__main__":
    main()
