import sys
from PIL import Image, ImageChops

MASKS = [(380, 0, 660, 32)]
TOLERANCE = 24

reference = Image.open(sys.argv[1]).convert("RGB")
current = Image.open(sys.argv[2]).convert("RGB")
if reference.size != current.size:
    print("100.0")
    sys.exit(0)
diff = ImageChops.difference(reference, current).convert("L").point(lambda v: 255 if v > TOLERANCE else 0)
for box in MASKS:
    diff.paste(0, box)
total = reference.size[0] * reference.size[1]
changed = diff.histogram()[255]
diff.save(sys.argv[3])
print("%.4f" % (changed * 100.0 / total))
