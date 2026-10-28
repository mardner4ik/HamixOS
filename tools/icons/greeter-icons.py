import math
import os
import sys
from PIL import Image, ImageDraw, ImageFont

ROOT = sys.argv[1] if len(sys.argv) > 1 else "data/overlay/usr/share/icons/HamixOS"
SIZES = [16, 24, 32, 48]
WHITE = (238, 238, 236, 255)


def canvas(size):
    scale = 8
    image = Image.new("RGBA", (size * scale, size * scale), (0, 0, 0, 0))
    return image, ImageDraw.Draw(image), size * scale


def power(draw, n):
    w = max(2, n // 9)
    box = (n * 0.18, n * 0.18, n * 0.82, n * 0.82)
    draw.arc(box, start=-60, end=240, fill=WHITE, width=w)
    draw.line((n / 2, n * 0.1, n / 2, n * 0.5), fill=WHITE, width=w)


def refresh(draw, n):
    w = max(2, n // 9)
    box = (n * 0.18, n * 0.18, n * 0.82, n * 0.82)
    draw.arc(box, start=20, end=320, fill=WHITE, width=w)
    tip = (n * 0.82, n * 0.5)
    draw.polygon([(tip[0] - n * 0.16, tip[1] - n * 0.2), (tip[0] + n * 0.12, tip[1] - n * 0.24), (tip[0] + n * 0.02, tip[1] + n * 0.04)], fill=WHITE)


def gear(draw, n):
    c = n / 2
    for k in range(8):
        a = k * math.pi / 4
        x, y = c + math.cos(a) * n * 0.36, c + math.sin(a) * n * 0.36
        draw.ellipse((x - n * 0.09, y - n * 0.09, x + n * 0.09, y + n * 0.09), fill=WHITE)
    draw.ellipse((c - n * 0.3, c - n * 0.3, c + n * 0.3, c + n * 0.3), fill=WHITE)
    draw.ellipse((c - n * 0.12, c - n * 0.12, c + n * 0.12, c + n * 0.12), fill=(0, 0, 0, 0))


def person(draw, n):
    c = n / 2
    draw.ellipse((c - n * 0.11, n * 0.06, c + n * 0.11, n * 0.28), fill=WHITE)
    w = max(2, n // 9)
    draw.line((n * 0.16, n * 0.38, n * 0.84, n * 0.38), fill=WHITE, width=w)
    draw.line((c, n * 0.36, c, n * 0.62), fill=WHITE, width=w)
    draw.line((c, n * 0.6, n * 0.3, n * 0.92), fill=WHITE, width=w)
    draw.line((c, n * 0.6, n * 0.7, n * 0.92), fill=WHITE, width=w)


def letter(text):
    def paint(draw, n):
        draw.rounded_rectangle((n * 0.06, n * 0.06, n * 0.94, n * 0.94), radius=n * 0.2, outline=WHITE, width=max(2, n // 12))
        font = None
        for path in ["/usr/share/fonts/TTF/DejaVuSans-Bold.ttf", "/usr/share/fonts/dejavu/DejaVuSans-Bold.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"]:
            if os.path.exists(path):
                font = ImageFont.truetype(path, int(n * 0.62))
                break
        font = font or ImageFont.load_default()
        box = draw.textbbox((0, 0), text, font=font)
        draw.text(((n - (box[2] - box[0])) / 2 - box[0], (n - (box[3] - box[1])) / 2 - box[1]), text, font=font, fill=WHITE)
    return paint


def avatar(draw, n):
    draw.ellipse((0, 0, n - 1, n - 1), fill=(120, 130, 140, 255))
    c = n / 2
    draw.ellipse((c - n * 0.17, n * 0.18, c + n * 0.17, n * 0.52), fill=WHITE)
    draw.chord((n * 0.18, n * 0.56, n * 0.82, n * 1.15), start=180, end=360, fill=WHITE)


ICONS = {
    "actions": {"system-shutdown": power, "system-shutdown-symbolic": power, "view-refresh-symbolic": refresh, "document-properties": gear, "document-properties-symbolic": gear},
    "apps": {"preferences-desktop-accessibility": person, "preferences-desktop-accessibility-symbolic": person, "nook_badge-symbolic": letter("N"), "enlightenment_badge-symbolic": letter("E"), "avatar-default": avatar},
}

dirs = []
for size in SIZES:
    for context, icons in ICONS.items():
        folder = os.path.join(ROOT, f"{size}x{size}", context)
        os.makedirs(folder, exist_ok=True)
        dirs.append((f"{size}x{size}/{context}", size, context))
        for name, paint in icons.items():
            image, draw, n = canvas(size)
            paint(draw, n)
            image.resize((size, size), Image.LANCZOS).save(os.path.join(folder, name + ".png"))

with open(os.path.join(ROOT, "index.theme"), "w") as index:
    index.write("[Icon Theme]\nName=HamixOS\nComment=HamixOS icons for the login screen\nInherits=Nook Icons,hicolor\n")
    index.write("Directories=" + ",".join(d for d, _, _ in dirs) + "\n")
    for d, size, context in dirs:
        index.write(f"\n[{d}]\nSize={size}\nContext={'Actions' if context == 'actions' else 'Applications'}\nType=Fixed\n")
