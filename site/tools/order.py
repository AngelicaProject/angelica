"""Work out the order in which the lily is drawn.

Usage: python -I order.py <art folder>

Reads solid.png (the flower's silhouette) and writes two maps the size of the
silhouette's frame, each a black image whose alpha says how early a point is
reached (opaque = first, faint = last, clear = not part of the flower):

  ground.png  travelling up through the plant from where the stems meet the
              ground, so it grows: stems, heads, then out along every stamen
  heart.png   travelling through the plant from the heart of the main head,
              so ink spreads: tepals, stamens, the second head, down the stems
"""
import sys
from collections import deque
from pathlib import Path

import numpy as np
from PIL import Image, ImageFilter

art = Path(sys.argv[1])
solid = Image.open(art / "solid.png").getchannel("A")
W = 480
H = round(solid.height * W / solid.width)
# A little wider than the silhouette, so the outline and thin stamens are inside it.
inside = np.asarray(solid.resize((W, H), Image.LANCZOS).filter(ImageFilter.MaxFilter(5))) > 60


def travel(seeds):
    """Distance from the seeds to every point, moving only through the plant."""
    dist = np.full((H, W), np.inf, dtype=np.float32)
    queue = deque()
    for y, x in seeds:
        if inside[y, x]:
            dist[y, x] = 0
            queue.append((y, x))
    steps = [(-1, 0, 1), (1, 0, 1), (0, -1, 1), (0, 1, 1), (-1, -1, 1.414), (-1, 1, 1.414), (1, -1, 1.414), (1, 1, 1.414)]
    while queue:  # a queue is enough here: step costs are close to equal
        y, x = queue.popleft()
        d = dist[y, x]
        for dy, dx, cost in steps:
            ny, nx = y + dy, x + dx
            if 0 <= ny < H and 0 <= nx < W and inside[ny, nx] and d + cost < dist[ny, nx]:
                dist[ny, nx] = d + cost
                queue.append((ny, nx))
    reached = np.isfinite(dist)
    far = dist[reached].max()
    # Parts cut off from the seeds (a stray stamen tip) come last rather than never.
    dist = np.where(inside & ~reached, far, dist)
    return np.where(inside, dist / far, 0), reached.sum() / inside.sum()


def save(order, name, gamma):
    alpha = np.where(inside, 255 - (order ** gamma) * 200, 0).astype(np.uint8)
    image = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    image.putalpha(Image.fromarray(alpha).filter(ImageFilter.GaussianBlur(0.6)))
    image.save(art / name, optimize=True)
    print(name, (W, H), (art / name).stat().st_size)


ground, part = travel([(H - 1, x) for x in range(W)] + [(H - 2, x) for x in range(W)])
print("reached from the ground:", round(float(part), 3))
save(ground, "ground.png", 0.8)
heart, part = travel([(round(0.36 * H) + dy, round(0.49 * W) + dx) for dy in range(-4, 5) for dx in range(-4, 5)])
print("reached from the heart:", round(float(part), 3))
save(heart, "heart.png", 0.9)
