import sys
from PIL import Image

Image.open(sys.argv[1]).convert("RGB").save(sys.argv[2], optimize=True)
