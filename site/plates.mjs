// Blueprint drawings for the feature plates of the home page.
const f = (n) => n.toFixed(1);
const rad = (d) => (d * Math.PI) / 180;
const polar = (cx, cy, r, a) => [cx + r * Math.cos(a), cy + r * Math.sin(a)];
const line = (x1, y1, x2, y2, w = .9, o = 1, extra = "") => `<line x1="${f(x1)}" y1="${f(y1)}" x2="${f(x2)}" y2="${f(y2)}" stroke-width="${w}" opacity="${o}" ${extra}/>`;
const dot = (x, y, r = 3) => `<circle class="hollow" cx="${f(x)}" cy="${f(y)}" r="${r}" stroke-width=".9"/>`;
const warm = (x, y, r = 4.5) => `<circle class="warm" cx="${f(x)}" cy="${f(y)}" r="${r}"/>`;
const word = (x, y, s, anchor = "middle") => `<text class="t" x="${f(x)}" y="${f(y)}" text-anchor="${anchor}">${s}</text>`;
const draw = (cls, box, body) => `<svg class="${cls}" viewBox="${box}" aria-hidden="true"><g stroke="currentColor" fill="none">${body}</g></svg>`;


// Plate 1: the sandbox. The plugin calls Angelica and nothing else. Angelica checks what the player granted, then acts on the game or reaches an approved host. There is no way to the player's files.
function sandbox() {
  const o = [`<rect x="30" y="70" width="180" height="180" rx="6" stroke-width="1.2" stroke-dasharray="6 5"/>`, `<rect x="78" y="128" width="84" height="64" stroke-width="1.4"/>`];
  for (let i = 0; i < 4; i++) o.push(line(90, 144 + i * 11, 150 - (i % 2) * 18, 144 + i * 11, .9, .8));
  o.push(word(120, 60, "sandbox"), word(120, 212, "plugin"));
  o.push(line(162, 160, 258, 160, .9), `<rect x="258" y="92" width="14" height="150" stroke-width="1.4"/>`, word(265, 80, "angelica"));
  o.push(line(265, 242, 265, 274, .9), warm(265, 280, 5.5), word(280, 284, "you grant permissions", "start"));
  [[112, "the game"], [170, "internet"]].forEach(([y, name]) => o.push(line(272, y, 430, y, .9), dot(438, y, 4), word(438, y + 22, name)));
  o.push(line(272, 228, 344, 228, .9), line(344, 220, 344, 236, 1.6), dot(438, 228, 4), word(438, 250, "your files"));
  return draw("plate-art", "0 0 520 320", o.join(""));
}

// Plate 2: the game map. The hollow, dotted drawing is where things were before a patch. The solid one, with the names, is where they are after; on the page it is drawn moving there (see `moving.map`), and `rest` is the same thing standing still.
const mapPoints = [[90, 96, 34, 20, "player"], [214, 70, -22, 30, "inventory"], [330, 120, 28, -18, "chat"], [150, 200, 26, 26, "zone"], [300, 226, -30, 14, "target"], [420, 190, 20, 24, "party"]];
const mapEdges = [[0, 1], [1, 2], [0, 3], [3, 4], [2, 4], [2, 5], [4, 5]];
function map() {
  const pts = mapPoints, o = [], rest = [];
  mapEdges.forEach(([a, b]) => { o.push(line(pts[a][0], pts[a][1], pts[b][0], pts[b][1], .7, .45, 'stroke-dasharray="2 5"')); rest.push(line(pts[a][0] + pts[a][2], pts[a][1] + pts[a][3], pts[b][0] + pts[b][2], pts[b][1] + pts[b][3], .9, .9)); });
  pts.forEach(([x, y, dx, dy, s]) => { o.push(dot(x, y, 3.2)); rest.push(`<circle cx="${x + dx}" cy="${y + dy}" r="3.6" fill="currentColor"/>`, word(x + dx, y + dy + (dy > 0 ? 20 : -12), s)); });
  o.push(dot(70, 292, 3.2), word(82, 296, "before the patch", "start"), `<circle cx="250" cy="292" r="3.6" fill="currentColor"/>`, word(262, 296, "after", "start"));
  return draw("plate-art", "0 0 520 320", o.join("") + `<g class="rest">${rest.join("")}</g>`);
}

// Plate 3: describe, build, test, and the one step that is always yours.
function making() {
  const cx = 260, cy = 160, r = 96, o = [`<circle cx="${cx}" cy="${cy}" r="${r}" stroke-width="1.2"/>`, `<circle cx="${cx}" cy="${cy}" r="${r - 26}" stroke-width=".7" stroke-dasharray="2 6"/>`];
  for (let i = 0; i < 60; i++) { const a = (i / 60) * Math.PI * 2, [x1, y1] = polar(cx, cy, r, a), [x2, y2] = polar(cx, cy, r + (i % 5 ? 5 : 10), a); o.push(line(x1, y1, x2, y2, .8, .8)); }
  [[-90, "describe", 0, -20], [0, "build", 22, 4], [90, "test", 0, 30], [180, "approve", -22, 4]].forEach(([d, s, dx, dy], i) => { const [x, y] = polar(cx, cy, r, rad(d)); o.push(i === 3 ? warm(x, y, 6) : `<circle cx="${f(x)}" cy="${f(y)}" r="4.5" fill="currentColor"/>`, word(x + dx, y + dy, s, dx > 0 ? "start" : dx < 0 ? "end" : "middle"));
    const [sx, sy] = polar(cx, cy, r, rad(d + 14)), [ex, ey] = polar(cx, cy, r, rad(d + 76)); o.push(`<path d="M${f(sx)} ${f(sy)} A${r} ${r} 0 0 1 ${f(ex)} ${f(ey)}" stroke-width="3"/>`); });
  o.push(`<rect x="${cx - 26}" y="${cy - 18}" width="52" height="36" stroke-width="1.3"/>`, line(cx - 16, cy - 6, cx + 16, cy - 6, .9), line(cx - 16, cy + 4, cx + 6, cy + 4, .9));
  return draw("plate-art", "0 0 520 320", o.join(""));
}

// Plate 4: what is open. Left, the source; middle, the documented formats and plugin API; right, a request on GitHub and its answer. On the page a request crosses to the API document and becomes a line there (see `moving.sheets`).
function sheets() {
  const o = [];
  [40, 195, 350].forEach((x) => o.push(`<rect class="paper" x="${x}" y="62" width="130" height="176" stroke-width="1.2"/>`));
  // source: indented lines, as code reads
  [[0, 62], [14, 78], [14, 54], [28, 66], [28, 44], [14, 70], [0, 40], [0, 58], [14, 76]].forEach(([indent, len], i) => o.push(line(54 + indent, 84 + i * 16, 54 + indent + len * 0.9, 84 + i * 16, .9, .8)));
  // formats and API: a title, then entries
  o.push(line(209, 84, 275, 84, 2.2));
  [96, 70, 88, 60, 92, 76].forEach((len, i) => o.push(line(209, 106 + i * 16, 209 + len, 106 + i * 16, .9, .8)));
  // requests: a question, and under it the answer
  o.push(`<rect x="362" y="78" width="92" height="40" rx="6" stroke-width="1"/>`, line(372, 92, 440, 92, .9, .8), line(372, 104, 420, 104, .9, .8));
  o.push(`<rect x="376" y="134" width="92" height="40" rx="6" stroke-width="1"/>`, line(386, 148, 452, 148, .9, .8), `<path d="M386 162 l5 5 l9 -10" stroke-width="1.6"/>`, line(408, 160, 444, 160, .9, .8));
  o.push(`<path d="M350 206 H330 m6 -5 l-6 5 l6 5" stroke-width="1"/>`);
  o.push(word(105, 262, "source code"), word(260, 262, "formats and API"), word(415, 262, "API requests"));
  return draw("plate-art", "0 0 520 320", o.join(""));
}

export { sandbox, map, making, sheets, mapPoints, mapEdges };
