"""Prepare the lily for the two-column hero: thinner stems, a tighter crop,
and every rendering registered to the same frame.

Usage: python -I remake.py <photo> <out folder>

Writes thin.png (the photograph with its stems narrowed), then, all at the same
crop: outline.png (line drawing, 1.5x), solid.png (flat silhouette), and
heads.json with the region that counts as stems, in fractions of the frame.
The tone prints are made from thin.png by treat.py.
"""
import json
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageFilter, ImageOps

photo, out = Path(sys.argv[1]), Path(sys.argv[2])
out.mkdir(parents=True, exist_ok=True)
CROP = (0.02, 0.06, 0.98, 0.72)  # left, top, right, bottom of the photograph
SQUEEZE = 0.6                    # stems keep this much of their width

src = np.asarray(Image.open(photo).convert("L"), dtype=np.float32)
H, W = src.shape
thin = src.copy()
# Each stem: where it starts, where it leaves the frame, and the row it begins at.
stems = [((492, 548), (482, H - 1)), ((668, 852), (628, H - 1)), ((372, 968), (425, H - 1))]
lines = []
for (x0, y0), (x1, y1) in stems:
    for y in range(y0, y1 + 1):
        guess = x0 + (x1 - x0) * (y - y0) / (y1 - y0)
        lo, hi = int(guess - 34), int(guess + 34)
        window = src[y, lo:hi]
        if window.sum() < 1:
            continue
        centre = lo + float((window * np.arange(hi - lo)).sum() / window.sum())
        xs = np.arange(lo, hi)
        sample = centre + (xs - centre) / SQUEEZE
        inside = (sample > lo) & (sample < hi - 1)
        ease = min(1.0, (y - y0) / 40)  # the narrowing comes in gradually below the head
        squeezed = np.where(inside, np.interp(sample, np.arange(lo, hi), window), 0)
        thin[y, lo:hi] = window * (1 - ease) + squeezed * ease
        lines.append((y, centre))
Image.fromarray(np.clip(thin, 0, 255).astype(np.uint8)).save(out / "thin.png")

l, t, r, b = CROP
box = (round(l * W), round(t * H), round(r * W), round(b * H))
frame = ImageOps.autocontrast(Image.fromarray(np.clip(thin, 0, 255).astype(np.uint8)).crop(box), cutoff=(2, 0.5))
print("frame", frame.size, "aspect", round(frame.width / frame.height, 4))


def save(alpha, name, levels=5):
    step = 255 / levels
    image = Image.new("RGBA", alpha.size, (0, 0, 0, 0))
    image.putalpha(alpha.point(lambda v: int(round(v / step) * step)))
    image.save(out / name, optimize=True)
    print(name, alpha.size, (out / name).stat().st_size)


big = frame.resize((1050, round(1050 * frame.height / frame.width)), Image.LANCZOS)
soft = big.filter(ImageFilter.GaussianBlur(1.9))
shape = soft.point(lambda v: 255 if v > 46 else 0).filter(ImageFilter.GaussianBlur(1.1)).point(lambda v: 255 if v > 127 else 0)
rim = np.asarray(shape.filter(ImageFilter.MaxFilter(5)), dtype=np.int16) - np.asarray(shape.filter(ImageFilter.MinFilter(3)), dtype=np.int16)
inner = (np.asarray(big.filter(ImageFilter.GaussianBlur(2.6)).filter(ImageFilter.FIND_EDGES), dtype=np.float32) > 20) & (np.asarray(shape) > 0)
save(Image.fromarray(np.where((rim > 0) | inner, 255, 0).astype(np.uint8)).filter(ImageFilter.GaussianBlur(0.6)), "outline.png")
# The same drawing with the inside of the flower worked out: petal edges and creases, as thinner lines.
fine = (np.asarray(big.filter(ImageFilter.GaussianBlur(1.3)).filter(ImageFilter.FIND_EDGES), dtype=np.float32) > 30) & (np.asarray(shape.filter(ImageFilter.MinFilter(5))) > 0)
detail = np.where((rim > 0) | inner, 255, np.where(fine, 190, 0)).astype(np.uint8)
save(Image.fromarray(detail).filter(ImageFilter.GaussianBlur(0.6)), "detail.png")
save(shape.resize((700, round(700 * frame.height / frame.width)), Image.LANCZOS), "solid.png", levels=3)

# Stem regions as polygons in fractions of the frame, for splitting the print into two inks.
fw, fh = box[2] - box[0], box[3] - box[1]
polys = []
for (x0, y0), (x1, y1) in stems:
    top, bottom = max(y0 + 6, box[1]), box[3]
    xa = x0 + (x1 - x0) * (top - y0) / (y1 - y0)
    xb = x0 + (x1 - x0) * (bottom - y0) / (y1 - y0)
    polys.append([[(xa - 16 - box[0]) / fw, (top - box[1]) / fh], [(xa + 16 - box[0]) / fw, (top - box[1]) / fh], [(xb + 16 - box[0]) / fw, 1], [(xb - 16 - box[0]) / fw, 1]])
# The bud belongs with the stems: it has not opened.
polys.append([[(330 - box[0]) / fw, (760 - box[1]) / fh], [(415 - box[0]) / fw, (760 - box[1]) / fh], [(415 - box[0]) / fw, (975 - box[1]) / fh], [(330 - box[0]) / fw, (975 - box[1]) / fh]])
(out / "stems.json").write_text(json.dumps(polys))
