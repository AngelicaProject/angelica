"""Print a photograph taken on black as a one-colour image, four ways.

Usage: python -I treat.py <photo> <out folder> [width] [crop]

`crop` is "left,top,right,bottom" as fractions of the photo; a tight crop turns
the flowers into large shapes that run off the edges.

Each result is an alpha mask of the light parts, to be printed in a pale
colour on a flat dark or saturated ground:
  lines.png   a line screen, lines swelling with brightness
  dots.png    a round-dot halftone at 45 degrees
  dither.png  an ordered 8x8 dither on a coarse pixel grid
  grain.png   a random grain, like a mezzotint
  flat.png    two flat tones cut from the shapes, with the creases left bare
  tones.png   four stepped tones, hatched, with the petal detail kept
"""
import os
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageFilter, ImageOps

photo, out = Path(sys.argv[1]), Path(sys.argv[2])
WIDTH = int(sys.argv[3]) if len(sys.argv) > 3 else 520
SS = 3  # supersampling, for smooth edges

src = Image.open(photo).convert("L")
if len(sys.argv) > 4:
    l, t, r, b = (float(v) for v in sys.argv[4].split(","))
    src = src.crop((round(l * src.width), round(t * src.height), round(r * src.width), round(b * src.height)))
src = ImageOps.autocontrast(src, cutoff=(2, 0.5))
HEIGHT = round(src.height * WIDTH / src.width)
big = src.resize((WIDTH * SS, HEIGHT * SS), Image.LANCZOS).filter(ImageFilter.GaussianBlur(0.8 * SS))
tone = (np.asarray(big, dtype=np.float32) / 255.0) ** 0.9
tone = np.where(tone < 0.06, 0, tone)  # the black ground stays bare
ys, xs = np.mgrid[0 : tone.shape[0], 0 : tone.shape[1]].astype(np.float32)


def rotated(angle):
    a = np.deg2rad(angle)
    return xs * np.cos(a) + ys * np.sin(a), -xs * np.sin(a) + ys * np.cos(a)


def lines(period=5.4, angle=32):
    u, _ = rotated(angle)
    return (0.5 + 0.5 * np.cos(2 * np.pi * u / (period * SS))) < np.clip(tone * 1.08, 0, 0.96)


def dots(period=float(os.environ.get("DOT_PERIOD", "6.2")), angle=45):
    u, v = rotated(angle)
    cell = 0.5 + 0.25 * (np.cos(2 * np.pi * u / (period * SS)) + np.cos(2 * np.pi * v / (period * SS)))
    return cell > 1 - np.clip(tone * 1.05, 0, 0.97)


BAYER = np.array([[0, 32, 8, 40, 2, 34, 10, 42], [48, 16, 56, 24, 50, 18, 58, 26], [12, 44, 4, 36, 14, 46, 6, 38], [60, 28, 52, 20, 62, 30, 54, 22],
                  [3, 35, 11, 43, 1, 33, 9, 41], [51, 19, 59, 27, 49, 17, 57, 25], [15, 47, 7, 39, 13, 45, 5, 37], [63, 31, 55, 23, 61, 29, 53, 21]], dtype=np.float32)


def dither(pixel=2):
    small = np.asarray(Image.fromarray((tone * 255).astype(np.uint8)).resize((WIDTH // pixel, HEIGHT // pixel), Image.BOX), dtype=np.float32) / 255.0
    h, w = small.shape
    threshold = (np.tile(BAYER, (h // 8 + 1, w // 8 + 1))[:h, :w] + 0.5) / 64.0
    hard = Image.fromarray(((small > threshold) * 255).astype(np.uint8)).resize((WIDTH * SS, HEIGHT * SS), Image.NEAREST)
    return np.asarray(hard) > 127


def grain(size=1.5):
    rng = np.random.default_rng(7)
    noise = rng.random((HEIGHT * 2, WIDTH * 2), dtype=np.float32)
    soft = Image.fromarray((noise * 255).astype(np.uint8)).filter(ImageFilter.GaussianBlur(size * 0.45)).resize((WIDTH * SS, HEIGHT * SS), Image.BICUBIC)
    field = np.asarray(ImageOps.equalize(soft), dtype=np.float32) / 255.0
    return field < np.clip(tone * 1.02, 0, 0.98)


def flat():
    soft = np.asarray(big.filter(ImageFilter.GaussianBlur(2.2 * SS)), dtype=np.float32) / 255.0
    edge = np.asarray(big.filter(ImageFilter.GaussianBlur(1.4 * SS)).filter(ImageFilter.FIND_EDGES), dtype=np.float32) / 255.0
    u, _ = rotated(32)
    half = (0.5 + 0.5 * np.cos(2 * np.pi * u / (4.6 * SS))) < 0.42  # the middle tone is a plain hatch
    return ((soft > 0.5) | ((soft > 0.2) & half)) & (edge < 0.06)


def tones():
    """Four steps: bare, a thin hatch, a heavy hatch, solid. Creases and veins are sharpened first so they survive."""
    sharp = np.asarray(big.filter(ImageFilter.UnsharpMask(radius=4 * SS, percent=160, threshold=2)), dtype=np.float32) / 255.0
    u, _ = rotated(32)
    wave = 0.5 + 0.5 * np.cos(2 * np.pi * u / (float(os.environ.get("HATCH_PERIOD", "4.4")) * SS))
    # Thin bright things, such as stamens, would break up under a hatch, so they print solid.
    thin = (tone - np.asarray(big.filter(ImageFilter.GaussianBlur(5 * SS)), dtype=np.float32) / 255.0) > 0.085
    return (sharp > 0.86) | ((sharp > 0.62) & (wave < 0.68)) | ((sharp > 0.38) & (wave < 0.42)) | ((sharp > 0.16) & (wave < 0.2)) | (thin & (tone > 0.2))


out.mkdir(parents=True, exist_ok=True)
for name, ink in (("lines", lines()), ("dots", dots()), ("dither", dither()), ("grain", grain()), ("flat", flat()), ("tones", tones())):
    alpha = Image.fromarray((ink * 255).astype(np.uint8)).resize((WIDTH, HEIGHT), Image.LANCZOS).point(lambda v: round(v / 51) * 51)
    picture = Image.new("RGBA", alpha.size, (0, 0, 0, 0))
    picture.putalpha(alpha)
    picture.save(out / f"{name}.png", optimize=True)
    print(name, alpha.size, (out / f"{name}.png").stat().st_size)
