// The home page with motion: the hero's lily is drawn from the prompt, and
// every section below answers to scrolling.
// Usage: node home.mjs <art folder> <output file> <status JSON from status.mjs>
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { sandbox, map, making, sheets, mapPoints, mapEdges } from "./plates.mjs";

const [art, out, feeds] = process.argv.slice(2);
const uri = (name) => `url(data:image/png;base64,${readFileSync(join(art, `${name}.png`)).toString("base64")})`;
const f = (n) => n.toFixed(1);
const ideas = ["shows where to gather any material I search for", "tells me when someone says my name in chat", "shows the recipe for any item I type", "reminds me an hour before the weekly reset"];

// The status under the buttons, from the feeds the build read (status.mjs). Dates are UTC and
// absolute: the page is built once and read until the next build.
const status = JSON.parse(feeds);
const months = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const day = (iso) => { const d = new Date(iso); return `${d.getUTCDate()} ${months[d.getUTCMonth()]}`; };
const [release, nightly] = status.angelica.version.split("-nightly.");
const facts = [
  ["Angelica", release, nightly ? `nightly ${nightly}` : "stable"],
  ["Map of the game", day(status.map.published), status.map.version],
  status.game ? ["For the game update", day(`${status.game.date}T00:00:00Z`), status.game.date.replaceAll("-", ".")] : ["For the game update", "None yet", "no build record"],
];

// A filter that turns an order map into ink: what has been reached becomes solid colour.
const ink = (id, [r, g, b]) => `<filter id="${id}" x="0" y="0" width="1" height="1" color-interpolation-filters="sRGB"><feComponentTransfer><feFuncR type="linear" slope="0" intercept="${r}"/><feFuncG type="linear" slope="0" intercept="${g}"/><feFuncB type="linear" slope="0" intercept="${b}"/><feFuncA type="linear" slope="18" intercept="-18"/></feComponentTransfer></filter>`;
// The lily: a line drawing, a pale hatch inside it, and the red print, each shown through its own filter.
function lily(id, line, fill, geometry) {
  const [cx, cy, r] = [344, 229, 334], ticks = [];
  for (let i = 0; i < 72; i++) { const a = (i / 72) * Math.PI * 2, long = i % 6 === 0, r2 = r + (long ? 14 : 7); ticks.push(`<line x1="${f(cx + r * Math.cos(a))}" y1="${f(cy + r * Math.sin(a))}" x2="${f(cx + r2 * Math.cos(a))}" y2="${f(cy + r2 * Math.sin(a))}" stroke-width="${long ? 1 : .6}"/>`); }
  return `<div class="art" aria-hidden="true">
    <svg class="defs" width="0" height="0"><defs>${ink(`line-${id}`, line)}${ink(`fill-${id}`, fill)}</defs></svg>
    <div class="print outline"><div class="ord" style="filter: url(#line-${id})"></div></div>
    <div class="print ghost"><div class="ord" style="filter: url(#line-${id})"></div></div>
    <div class="print fill"><div class="ord" style="filter: url(#fill-${id})"></div></div>
    ${geometry ? `<svg class="geo" viewBox="0 0 700 721"><g stroke="currentColor" fill="none"><g class="turn"><circle cx="${cx}" cy="${cy}" r="${r}" stroke-width=".9" stroke-dasharray="2 7"/>${ticks.join("")}</g><circle cx="478" cy="469" r="212" stroke-width=".8" stroke-dasharray="2 7" opacity=".8"/><circle class="warm" cx="620" cy="117" r="6"/></g></svg>` : ""}</div>`;
}

// Mark every stroke of a plate so it can be drawn in order: solid strokes are drawn along their length, dashed ones and marks fade in.
function drawable(svg) {
  let k = 0;
  return svg.replace(/<(line|path|circle|rect|text)\b([^>]*?)(\/?)>/g, (_, tag, attrs, close) => {
    const i = k++, solid = tag !== "text" && !/stroke-dasharray|class=|fill="currentColor"/.test(attrs);
    const cls = solid ? "ink" : "pop";
    attrs = /class="/.test(attrs) ? attrs.replace(/class="([^"]*)"/, `class="$1 ${cls}"`) : `${attrs} class="${cls}"`;
    return `<${tag}${attrs}${solid ? ' pathLength="1"' : ""} style="--i:${Math.min(i, 90)}"${close}>`;
  });
}

const feature = (n, kicker, title, body, picture, flip, name) => `<article class="feature${flip ? " flip" : ""}">
  <div class="say"><span class="kicker" data-r>${n} · ${kicker}</span><h3 data-r style="--d:1">${title}</h3><p data-r style="--d:2">${body}</p></div>
  <div class="plate" data-r style="--d:1"><div class="grid"></div>${alive(drawable(picture), name)}</div></article>`;
// A heading whose words rise one after another; the second part is set in italic.
const rise = (plain, italic) => { let i = 0; const part = (text) => text.split(" ").map((w) => `<span class="w"><span style="--i:${i++}">${w}</span></span>`).join(" "); return `${part(plain)} <i>${part(italic)}</i>`; };
// What keeps moving on a plate once it is drawn: small warm points, one idea each.
const moving = {
  sandbox: [["M162 160 H258 L272 112 H430", 0, "0;0.3;0.42;1"], ["M162 160 H258 L272 170 H430", 1.5, "0;0.3;0.42;1"]].map(([path, begin, times]) => `<circle r="3.4"><animateMotion path="${path}" dur="4.4s" begin="${begin}s" keyPoints="0;0.33;0.4;1" keyTimes="${times}" calcMode="linear" repeatCount="indefinite"/></circle>`).join("")
    + `<circle r="3.4"><animateMotion path="M162 160 H258 L272 228 H340 H272 L258 160 H162" dur="5.6s" begin="3s" repeatCount="indefinite"/></circle><circle cx="265" cy="280" r="5.5"><animate attributeName="r" values="5.5;9;5.5" dur="2.4s" repeatCount="indefinite"/></circle>`,
  // The solid drawing rests where things were, moves to where a patch puts them, rests, and returns.
  map: (() => {
    const shift = (before, after) => `values="${before};${before};${after};${after};${before}" keyTimes="0;0.28;0.42;0.9;1" dur="7s" repeatCount="indefinite"`;
    const at = (i, moved) => [mapPoints[i][0] + (moved ? mapPoints[i][2] : 0), mapPoints[i][1] + (moved ? mapPoints[i][3] : 0)];
    const edges = mapEdges.map(([a, b]) => { const [ax, ay] = at(a), [bx, by] = at(b), [cx, cy] = at(a, true), [dx, dy] = at(b, true); return `<line class="plain" x1="${ax}" y1="${ay}" x2="${bx}" y2="${by}"><animate attributeName="x1" ${shift(ax, cx)}/><animate attributeName="y1" ${shift(ay, cy)}/><animate attributeName="x2" ${shift(bx, dx)}/><animate attributeName="y2" ${shift(by, dy)}/></line>`; });
    const points = mapPoints.map(([x, y, dx, dy, name]) => { const label = dy > 0 ? 20 : -12; return `<circle class="plain" cx="${x}" cy="${y}" r="3.6"><animate attributeName="cx" ${shift(x, x + dx)}/><animate attributeName="cy" ${shift(y, y + dy)}/></circle><text class="t" x="${x}" y="${y + label}" text-anchor="middle"><animate attributeName="x" ${shift(x, x + dx)}/><animate attributeName="y" ${shift(y + label, y + dy + label)}/>${name}</text>`; });
    return edges.join("") + points.join("");
  })(),
  // Describe, build, test, approve: each quarter of the circle is drawn in turn, then the plugin in the middle switches on.
  making: [-90, 0, 90, 180].map((d, i) => { const p = (deg) => [260 + 96 * Math.cos((deg * Math.PI) / 180), 160 + 96 * Math.sin((deg * Math.PI) / 180)], [sx, sy] = p(d + 14), [ex, ey] = p(d + 76), from = (i * 1.5) / 8.4, to = (i * 1.5 + 1.3) / 8.4;
    return `<path class="arc" pathLength="1" stroke-dasharray="1" stroke-dashoffset="1" d="M${f(sx)} ${f(sy)} A96 96 0 0 1 ${f(ex)} ${f(ey)}"><animate attributeName="stroke-dashoffset" values="1;1;0;0;1" keyTimes="0;${from.toFixed(3)};${to.toFixed(3)};0.94;1" dur="8.4s" repeatCount="indefinite"/></path>`; }).join("")
    + `<rect class="lit" x="234" y="142" width="52" height="36"><animate attributeName="fill-opacity" values="0;0;0.55;0.55;0" keyTimes="0;0.72;0.78;0.94;1" dur="8.4s" repeatCount="indefinite"/></rect>`,
  // A request leaves the thread, reaches the API document, and a new line is written there.
  sheets: `<circle r="3.6"><animateMotion path="M420 176 V206 H305" dur="6.5s" keyPoints="0;0;1;1" keyTimes="0;0.12;0.45;1" calcMode="linear" repeatCount="indefinite"/><animate attributeName="opacity" values="0;1;1;0;0" keyTimes="0;0.12;0.45;0.5;1" dur="6.5s" repeatCount="indefinite"/></circle>`
    + `<path class="arc" pathLength="1" stroke-dasharray="1" stroke-dashoffset="1" d="M209 206 H297"><animate attributeName="stroke-dashoffset" values="1;1;0;0;1" keyTimes="0;0.45;0.62;0.94;1" dur="6.5s" repeatCount="indefinite"/></path>`,
};
const alive = (svg, name) => svg.replace(/<\/g><\/svg>$/, `<g class="live">${moving[name]}</g></g></svg>`);
// Angelica's own window, as it looks in the game (light theme).
const row = (cls, letter, name, note) => `<div class="row ${cls}"><b class="tile">${letter}</b><span class="name">${name}</span><span class="about">${note}</span><span class="act">Allow and turn on →</span><i class="state"></i></div>`;
const gameWindow = `<div class="win" data-state="0">
  <div class="wbar"><span class="wm">angelica</span><span class="status"><i></i><span class="s0">1 waits for you · 1 running</span><span class="s2">2 running</span></span><span class="x">×</span></div>
  <div class="tabs"><b>Installed</b><span>Catalog</span><span>Settings</span></div>
  <div class="sect wait"><span>Needs you</span><em>1</em></div>
  ${row("new", "G", "Gathering", "wants to see the items you carry")}
  <div class="sect"><span>On</span><em class="count">1</em></div>
  ${row("on", "H", "Harmonia", "translation")}
  ${row("on moved", "G", "Gathering", "sees the items you carry")}
</div>`;
const letters = (word) => [...word].map((c, i) => `<span style="--i:${i}">${c}</span>`).join("");
// What a plugin can do today, in the words the player approves: the host's permissions (docs/architecture/permissions.md) and the game map's domains.
const ticker = ["use actions for you", "teleport you", "answer the game's dialogs", "see your character", "see the items you carry", "read chat", "hide chat messages", "change the game's text", "change your camera's zoom limits", "hide parts of the game's windows", "replace fonts and textures", "draw over the game world", "show notifications", "play sounds", "add chat commands", "print to your chat log", "download from approved hosts"].join(" · ") + " · ";

const html = `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Angelica</title>
<link rel="preconnect" href="https://fonts.googleapis.com"><link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Cormorant+Garamond:ital,wght@0,300;0,400;0,500;1,300;1,400&family=Golos+Text:wght@400;500;600&family=JetBrains+Mono:wght@400;500;700&family=Manrope:wght@400;500;600;700&family=Quicksand:wght@600&display=swap">
<style>
/* Home page. Pearl paper and dawn, the italic word, blueprint drawings, one band of night ink, and a red lily that is drawn from what you type. Motion: things are drawn when they come into view, and a few follow the scroll itself. */
:root { --tones: ${uri("tones")}; --detail: ${uri("detail")}; --ground: ${uri("ground")};
  --pearl: #f4f2ee; --plate: #fbfaf8; --ink: #16202f; --ink-2: #5d6676; --line: #c9cdd4; --blueprint: #3f7fa8; --pale: #8fb7d6; --gold: #e3b768; --red: #a8432f; --dawn-1: #dfe7f1; --dawn-2: #f3dccb; --dawn-3: #f7ecd9;
  --soft: "Cormorant Garamond", "Times New Roman", serif; --sans: "Manrope", "Segoe UI", system-ui, sans-serif; --mono: "JetBrains Mono", Consolas, ui-monospace, monospace;
  --ease: cubic-bezier(.2, .7, .2, 1);
  --deep: #16202f; --card: #16202f; --card-text: #f4f2ee; --rule: rgba(22,32,47,.2); --on: #2e7d46; --wait: #b76e00; --bar: rgba(244,242,238,.92); --grid: rgba(63,127,168,.13); --ghost: #e3dfd7; color-scheme: light; }
* { box-sizing: border-box; }
html { scroll-behavior: smooth; }
body { margin: 0; background: var(--pearl); color: var(--ink); font-family: var(--sans); font-size: 17px; line-height: 1.6; overflow-x: hidden; }
a { color: inherit; }
.wrap { max-width: 1180px; margin: 0 auto; padding: 0 16px; }
.kicker { font-family: var(--mono); font-size: 12px; letter-spacing: .14em; text-transform: uppercase; color: var(--blueprint); display: block; }
h2 { font-family: var(--soft); font-weight: 300; font-size: clamp(44px, 6.4vw, 84px); line-height: .98; letter-spacing: -.01em; margin: 10px 0 0; text-wrap: balance; }
.grid { position: absolute; inset: 0; background-image: linear-gradient(var(--grid) 1px, transparent 1px), linear-gradient(90deg, var(--grid) 1px, transparent 1px); background-size: 48px 48px; }
.b { font-family: var(--mono); font-size: 12.5px; font-weight: 700; letter-spacing: .08em; text-transform: uppercase; padding: 13px 18px; border-radius: 2px; border: 0; flex: none; white-space: nowrap; text-decoration: none; color: inherit; cursor: pointer; transition: transform .15s, background .15s, color .15s; display: inline-block; }
.b.one { background: var(--ink); color: var(--pearl); }
.b.one:hover { background: var(--red); transform: translateY(-2px); }
.b.two { background: none; box-shadow: inset 0 0 0 1px currentColor; }
.b.two:hover { background: var(--ink); color: var(--pearl); }

/* Things that arrive when scrolled to. */
[data-r] { opacity: 0; transform: translateY(26px); transition: opacity .8s var(--ease), transform .8s var(--ease); transition-delay: calc(var(--d, 0) * 90ms); }
[data-r].in { opacity: 1; transform: none; }

h2[data-r] { opacity: 1; transform: none; }
h2 .w { display: inline-block; overflow: hidden; vertical-align: bottom; padding-bottom: .14em; margin-bottom: -.14em; }
h2 .w > span { display: inline-block; transform: translateY(112%); transition: transform 1s var(--ease) calc(var(--i) * 80ms); }
h2.in .w > span { transform: none; }
.key { display: inline-block; font: 600 .78em var(--mono); color: var(--ink); background: var(--plate); border: 1px solid var(--line); border-bottom-width: 3px; border-radius: 4px; padding: 1px 8px 0; vertical-align: .08em; animation: press 2.6s ease-in-out infinite; }
@keyframes press { 0%, 62%, 100% { transform: none; border-bottom-width: 3px; } 70%, 78% { transform: translateY(2px); border-bottom-width: 1px; background: var(--gold); } }

/* The page's progress, as a ruled line, and a small bar that stays once the hero is gone. */
.rule { position: fixed; z-index: 30; left: 0; top: 0; height: 3px; width: 100%; background: var(--red); transform-origin: 0 50%; transform: scaleX(0); will-change: transform; }
.dock { position: fixed; z-index: 20; left: 0; right: 0; top: 0; background: var(--bar); border-bottom: 1px solid var(--line); transform: translateY(-110%); transition: transform .45s var(--ease); }
.dock.on { transform: none; }
.dock .wrap { display: flex; align-items: center; gap: 24px; padding-top: 10px; padding-bottom: 8px; }
.dock .mark { font-family: var(--soft); font-style: italic; font-weight: 300; font-size: 26px; line-height: 1; text-decoration: none; margin-right: auto; }
.dock a.nav, .bar a.nav { font-size: 14px; font-weight: 600; text-decoration: none; color: var(--ink-2); transition: color .15s; }
.dock a.nav:hover, .bar a.nav:hover { color: var(--red); }
.dock .b { padding: 9px 14px; }
.bar { display: flex; align-items: center; gap: 26px; padding: 16px 0; flex-wrap: wrap; }
.bar .mark { font-family: var(--soft); font-style: italic; font-weight: 300; font-size: 28px; line-height: 1; text-decoration: none; margin-right: auto; }

/* Hero */
.hero { --grown: 0; position: relative; border-radius: 4px; overflow: hidden; min-height: 680px; display: grid; align-content: center; padding: 64px 56px; background: linear-gradient(180deg, var(--dawn-1) 0%, var(--dawn-2) 70%, var(--dawn-3) 100%); }
.hero > * { position: relative; }
.hero > .grid { position: absolute; }
.art { position: absolute !important; aspect-ratio: 984 / 1014; pointer-events: none; }
.hero .art { right: 5%; bottom: 150px; height: 600px; }
.print { position: absolute; inset: 0; -webkit-mask: var(--picture) center / contain no-repeat; mask: var(--picture) center / contain no-repeat; }
.print.outline { --picture: var(--detail); transform: translate(5px, 4px); }
.print.ghost { --picture: var(--tones); opacity: .3; }
.print.fill { --picture: var(--tones); }
.ord { position: absolute; inset: 0; background: var(--ground) center / 100% 100% no-repeat; }
.defs { position: absolute; }
.geo { position: absolute; inset: 0; width: 100%; height: 100%; overflow: visible; color: var(--blueprint); opacity: calc(.2 + .6 * var(--grown)); transition: opacity .5s; }
.geo .turn { transform-box: view-box; transform-origin: 344px 229px; animation: turn 120s linear infinite; }
.geo .warm { fill: var(--gold); stroke: var(--ink); stroke-width: .8; transform-box: fill-box; transform-origin: center; animation: pulse 2.6s ease-in-out infinite; opacity: var(--grown); }
@keyframes drift { from { transform: translate(8px, 6px); } to { transform: translate(3px, 2px); } }
@keyframes turn { to { transform: rotate(360deg); } }
@keyframes pulse { 50% { transform: scale(1.5); } }
@keyframes blink { 50% { opacity: 0; } }
.ground { position: absolute !important; left: 52%; right: 0; bottom: 186px; height: 9px; border-top: 1px solid var(--blueprint); opacity: .7; background: repeating-linear-gradient(90deg, var(--blueprint) 0 1px, transparent 1px 24px) top / auto 5px repeat-x; transform-origin: 100% 50%; animation: lay 1.4s var(--ease) both; }
@keyframes lay { from { transform: scaleX(0); } }
.mid { max-width: 560px; }
.mid > * { animation: arrive .9s var(--ease) both; }
.mid > :nth-child(2) { animation-delay: .1s; } .mid > :nth-child(3) { animation-delay: .2s; } .mid > :nth-child(4) { animation-delay: .3s; } .mid > :nth-child(5) { animation-delay: .4s; }
@keyframes arrive { from { opacity: 0; transform: translateY(22px); } }
.word { font-family: var(--soft); font-weight: 300; font-style: italic; font-size: clamp(72px, 13vw, 168px); line-height: .9; letter-spacing: -.01em; margin: 0; }
.lead { font-family: var(--soft); font-size: 30px; line-height: 1.15; margin: 22px 0 0; max-width: 20ch; }
.sub { margin: 12px 0 0; color: var(--ink-2); max-width: 42ch; }
.btns { display: flex; gap: 10px; flex-wrap: wrap; margin-top: 26px; }
/* Under the buttons: the three versions the release feeds gave, each named in plain words. The feeds do not say which game version is live, so nothing here claims Angelica is up to date with it. */
.facts { display: flex; margin: 22px 0 0; border-top: 1px solid var(--rule); max-width: 440px; }
.facts div { flex: 1; padding: 9px 14px 0 0; }
.facts span { display: block; font-size: 12.5px; color: var(--ink-2); }
.facts b { font-size: 15px; font-weight: 600; }
.facts small { display: block; font-family: var(--mono); font-size: 11px; color: var(--ink-2); margin-top: 1px; }
.card { position: absolute !important; right: 4%; bottom: 36px; width: 470px; height: 150px; background: var(--card); color: var(--card-text); border-radius: 3px; padding: 14px 16px 16px; display: grid; gap: 6px; align-content: start; animation: arrive .9s var(--ease) .35s both; }
.cardtop { display: flex; justify-content: space-between; align-items: baseline; }
.card .kicker { color: var(--pale); }
.copy { font: 500 13px var(--mono); color: var(--pale); background: none; border: 0; padding: 0; cursor: pointer; }
.copy:hover, .copy.done { color: var(--gold); }
.pre { margin: 2px 0 0; font: 400 12px/1.5 var(--mono); color: #8d9ab0; }
.idea { margin: 0; font: 500 16px/1.45 var(--mono); }
.field { outline: none; caret-color: var(--gold); min-width: 1ch; }
.field:focus { background: rgba(227,183,104,.16); }
.caret { display: inline-block; width: .5em; height: 1em; margin-left: 2px; vertical-align: -.12em; background: var(--gold); animation: blink 1s steps(1) infinite; }
.typing .caret { display: none; }

/* A line of the platform's own words, moving with the page. */
.ticker[hidden] { display: none; }
.ticker { margin: 34px 0 0; overflow: hidden; border-top: 1px solid var(--line); border-bottom: 1px solid var(--line); padding: 11px 0; white-space: nowrap; font-family: var(--mono); font-size: 12.5px; letter-spacing: .16em; text-transform: uppercase; color: var(--ink-2); }
.ticker div { display: inline-block; will-change: transform; }

section { margin-top: 120px; }
.head { max-width: 760px; }
.head p { color: var(--ink-2); font-size: 19px; margin: 18px 0 0; max-width: 52ch; }
.head code { font-family: var(--mono); font-size: .88em; color: var(--ink); }
.features { display: grid; gap: 28px; margin-top: 48px; }
.feature { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1.15fr); gap: 20px 56px; align-items: center; border-top: 1px solid var(--line); padding-top: 28px; }
.feature.flip { grid-template-columns: minmax(0, 1.15fr) minmax(0, 1fr); }
.feature.flip .say { order: 2; }
.feature h3 { font-family: var(--soft); font-weight: 400; font-size: clamp(32px, 3.6vw, 46px); line-height: 1.05; margin: 12px 0 14px; text-wrap: balance; }
.feature p { margin: 0; color: var(--ink-2); max-width: 44ch; }
.plate { position: relative; overflow: hidden; border: 1px solid var(--line); border-radius: 4px; background: var(--plate); aspect-ratio: 520 / 320; transition: opacity .8s var(--ease), transform .8s var(--ease), border-color .3s; }
.plate:hover { border-color: var(--blueprint); }
.plate { perspective: 900px; }
.plate-art { position: absolute; inset: 0; width: 100%; height: 100%; color: var(--blueprint); transform: rotateX(var(--rx, 0deg)) rotateY(var(--ry, 0deg)) scale(1.04); transition: transform .3s ease-out; }
.live { opacity: 0; transition: opacity .8s 1.8s; fill: var(--gold); stroke: var(--ink); stroke-width: .8; }
.plate.in .live { opacity: 1; }
.live .plain { fill: currentColor; stroke: currentColor; stroke-width: .9; }
.live .arc { fill: none; stroke: var(--gold); stroke-width: 3.6; stroke-linecap: round; }
.live .lit { stroke: none; fill-opacity: 0; }
.plate .rest { display: none; }
.plate-art .t { fill: currentColor; stroke: none; font-family: var(--mono); font-size: 11px; letter-spacing: .1em; }
.plate-art .warm { fill: var(--gold); stroke: var(--ink); stroke-width: .8; }
.plate-art .hollow, .plate-art .paper { fill: var(--plate); }
/* A plate is drawn when it comes into view: solid lines along their length, the rest fading in after. */
.ink { stroke-dasharray: 1; stroke-dashoffset: 1; transition: stroke-dashoffset 1.2s var(--ease) calc(var(--i) * 16ms + .2s); }
.pop { stroke-opacity: 0; fill-opacity: 0; transition: stroke-opacity .6s calc(var(--i) * 16ms + .5s), fill-opacity .6s calc(var(--i) * 16ms + .5s); }
.plate.in .ink { stroke-dashoffset: 0; } .plate.in .pop { stroke-opacity: 1; fill-opacity: 1; }
@media (max-width: 820px) { .feature, .feature.flip { grid-template-columns: minmax(0, 1fr); } .feature.flip .say { order: 0; } }

/* The window from the game, living its own short story: a plugin waits, the player allows it, it runs. */
.split { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1.1fr); gap: 20px 56px; align-items: center; }
.stage { position: relative; overflow: hidden; min-height: 440px; border-radius: 4px; background: linear-gradient(180deg, var(--dawn-1), var(--dawn-2) 75%, var(--dawn-3)); display: grid; place-items: center; padding: 56px 24px 32px; }
.note { position: absolute; right: 18px; top: 16px; display: flex; align-items: center; gap: 8px; background: rgba(246,243,237,.97); border: 1px solid rgba(60,45,20,.2); border-radius: 6px; padding: 6px 10px; font: 500 12.5px "Golos Text", "Segoe UI", sans-serif; color: rgba(36,33,28,.7); box-shadow: 0 8px 20px rgba(22,32,47,.12); animation: bob 5s ease-in-out infinite; transition: opacity .4s; }
.note i { width: 7px; height: 7px; border-radius: 50%; background: #b76e00; animation: blink 1.6s steps(1) infinite; }
@keyframes bob { 50% { transform: translateY(4px); } }
.wm { font-family: "Quicksand", sans-serif; font-weight: 600; font-size: 13px; color: rgba(36,33,28,.6); }
.win { position: relative; width: min(420px, 100%); background: rgba(246,243,237,.97); color: #24211c; border: 1px solid rgba(60,45,20,.2); border-radius: 6px; box-shadow: 0 18px 40px rgba(22,32,47,.2); font-family: "Golos Text", "Segoe UI", sans-serif; font-size: 13.5px; line-height: 1.3; overflow: hidden; animation: bob 7s ease-in-out -2s infinite; }
.win .wbar { height: 32px; display: flex; align-items: center; gap: 10px; padding: 0 10px; border-bottom: 1px solid rgba(60,45,20,.14); }
.win .wm { margin-right: auto; }
.win .status { display: flex; align-items: center; gap: 6px; color: rgba(36,33,28,.6); font-size: 12.5px; }
.win .status i { width: 7px; height: 7px; border-radius: 50%; background: #b76e00; transition: background .4s; }
.win .x { color: rgba(36,33,28,.42); font-size: 16px; }
.win .tabs { display: flex; gap: 18px; padding: 9px 12px 8px; border-bottom: 1px solid rgba(60,45,20,.14); color: rgba(36,33,28,.6); }
.win .tabs b { color: #24211c; font-weight: 600; }
.win .sect { display: flex; align-items: center; gap: 8px; padding: 9px 12px 4px; font-size: 11.5px; color: rgba(36,33,28,.6); transition: height .4s, opacity .4s, padding .4s; overflow: hidden; height: 30px; }
.win .sect::after { content: ""; flex: 1; height: 1px; background: rgba(60,45,20,.14); }
.win .sect em { font-style: normal; }
.win .row { height: 36px; display: flex; align-items: center; gap: 10px; padding: 0 12px; transition: background .3s, opacity .4s, height .4s; overflow: hidden; }
.win .tile { width: 22px; height: 22px; border-radius: 5px; background: rgba(122,92,46,.14); color: #7a5c2e; font-weight: 600; font-size: 12px; display: grid; place-items: center; flex: none; }
.win .name { font-weight: 500; }
.win .about { color: rgba(36,33,28,.6); font-size: 12.5px; margin-left: auto; }
.win .act { display: none; margin-left: auto; color: #7a5c2e; font-weight: 600; }
.win .state { display: flex; align-items: baseline; gap: 9px; margin: 22px 0 0; font-size: 15.5px; font-weight: 600; width: 8px; height: 8px; border-radius: 50%; background: #2e7d46; flex: none; }
.win .new .state { background: none; width: auto; height: auto; }
.win .new .state::after { content: "!"; color: #b76e00; font-weight: 600; font-style: normal; }
.win .moved { height: 0; opacity: 0; }
.win .s2 { display: none; }
.win[data-state="1"] .new { background: rgba(255,255,255,.75); }
.win[data-state="1"] .new .about { display: none; } .win[data-state="1"] .new .act { display: block; }
.win[data-state="2"] .new, .win[data-state="2"] .wait { height: 0; opacity: 0; padding-top: 0; padding-bottom: 0; }
.win[data-state="2"] .moved { height: 36px; opacity: 1; background: rgba(46,125,70,.08); }
.win[data-state="2"] .status i { background: #2e7d46; } .win[data-state="2"] .s0 { display: none; } .win[data-state="2"] .s2 { display: inline; }
.stage.calm .note { opacity: 0; }
@media (max-width: 820px) { .split { grid-template-columns: minmax(0, 1fr); } }

/* The night band: its lily is drawn by the scroll itself, and the three steps light up along the way. */
.night { --p: 0; position: relative; overflow: hidden; margin-top: 120px; border-radius: 4px; background: var(--deep); color: #f4f2ee; padding: 72px 56px 64px; }
.night .grid { background-image: linear-gradient(rgba(143,183,214,.09) 1px, transparent 1px), linear-gradient(90deg, rgba(143,183,214,.09) 1px, transparent 1px); transform: translateY(calc(var(--p) * -24px)); inset: -24px 0; }
.night > * { position: relative; }
.night > .grid { position: absolute; }
/* The lily stands on the band's bottom edge, whole, with room to its right; its stems leave through the edge instead of ending in mid-air. */
.night .art { right: 4%; top: auto; bottom: -3px; height: 640px; opacity: .92; }
.night .print.outline { transform: none; }
.night .kicker { color: var(--pale); }
.night .lede { color: #b9c4d3; font-size: 19px; max-width: 46ch; margin: 18px 0 0; }
.night h2 { max-width: 7.6em; }
.turns { display: grid; gap: 0; margin-top: 44px; max-width: 560px; position: relative; padding-left: 26px; }
.turns::before { content: ""; position: absolute; left: 0; top: 6px; bottom: 6px; width: 1px; background: #2a3547; }
.turns::after { content: ""; position: absolute; left: 0; top: 6px; bottom: 6px; width: 1px; background: var(--gold); transform-origin: 50% 0; transform: scaleY(var(--p)); }
.turns div { padding: 0 0 26px; opacity: .35; transition: opacity .5s; position: relative; }
.turns div::before { content: ""; position: absolute; left: -30px; top: 16px; width: 9px; height: 9px; border-radius: 50%; background: var(--ink); box-shadow: 0 0 0 1px #4a5870; transition: background .4s, box-shadow .4s; }
.turns div.on { opacity: 1; }
.turns div.on::before { background: var(--gold); box-shadow: 0 0 0 1px var(--gold); }
.turns span { font-family: var(--mono); font-size: 12px; letter-spacing: .14em; color: var(--pale); }
.turns h3 { font-family: var(--soft); font-weight: 400; font-style: italic; font-size: 38px; line-height: 1; margin: 6px 0 8px; }
.turns p { margin: 0; color: #b9c4d3; font-size: 16px; max-width: 40ch; }
.turns .yours.on h3 { color: #e0715a; }
.night .more { display: inline-block; margin-top: 8px; font-family: var(--mono); font-size: 13px; letter-spacing: .06em; color: var(--pale); text-decoration: none; border-bottom: 1px solid #3a4a62; padding-bottom: 2px; transition: color .15s, border-color .15s; }
.night .more:hover { color: var(--gold); border-color: var(--gold); }

.steps { display: grid; grid-template-columns: repeat(auto-fit, minmax(250px, 1fr)); margin-top: 44px; position: relative; }
.steps::before { content: ""; position: absolute; left: 0; right: 0; top: 0; height: 1px; background: var(--blueprint); transform-origin: 0 50%; transform: scaleX(0); transition: transform 1.4s var(--ease); }
.steps.in::before { transform: none; }
.steps div { padding: 18px 28px 0 0; }
.steps i { display: block; font-family: var(--soft); font-style: italic; font-weight: 300; font-size: 96px; line-height: .9; color: var(--blueprint); animation: turn-red 7.5s linear infinite; }
.steps div:nth-child(2) i { animation-delay: 2.5s; } .steps div:nth-child(3) i { animation-delay: 5s; }
@keyframes turn-red { 0%, 30%, 100% { color: var(--blueprint); } 6%, 24% { color: var(--red); } }
.steps div:hover i { color: var(--red); }
.steps h3 { font-size: 19px; font-weight: 700; margin: 10px 0 4px; }
.steps p { margin: 0; color: var(--ink-2); font-size: 16px; }
.after { display: flex; gap: 14px 28px; flex-wrap: wrap; align-items: center; margin-top: 36px; }
.after p { margin: 0; color: var(--ink-2); font-size: 14.5px; max-width: 62ch; }

.shelves { display: grid; grid-template-columns: repeat(auto-fit, minmax(270px, 1fr)); gap: 14px; margin-top: 44px; }
.shelf { border: 1px solid var(--line); border-radius: 4px; background: var(--plate); padding: 24px 24px 12px; transition: opacity .8s var(--ease), transform .8s var(--ease), border-color .3s, box-shadow .3s; transition-delay: calc(var(--d, 0) * 90ms), calc(var(--d, 0) * 90ms), 0s, 0s; }
.shelf.in:hover { border-color: var(--blueprint); transform: translateY(-4px); box-shadow: 0 14px 30px rgba(22,32,47,.08); }
.shelf h3 { font-family: var(--soft); font-weight: 400; font-style: italic; font-size: 40px; line-height: 1; margin: 8px 0 16px; }
.shelf ul { list-style: none; margin: 0; padding: 0; border-top: 1px solid var(--line); }
.shelf li + li { border-top: 1px solid var(--line); }
.shelf a { display: flex; justify-content: space-between; gap: 12px; padding: 11px 0; text-decoration: none; font-size: 16px; transition: color .15s, padding .25s var(--ease); }
.shelf a::after { content: "→"; color: var(--blueprint); transition: transform .25s var(--ease), color .15s; }
.shelf a:hover { color: var(--red); padding-left: 6px; }
.shelf a:hover::after { transform: translateX(4px); color: var(--red); }

footer { margin-top: 120px; border-top: 1px solid var(--line); padding-top: 22px; overflow: hidden; }
footer .row { display: flex; gap: 8px 28px; flex-wrap: wrap; color: var(--ink-2); font-size: 14px; }
footer .big { font-family: var(--soft); font-style: italic; font-weight: 300; font-size: clamp(120px, 27vw, 380px); line-height: 1; letter-spacing: -.02em; margin: 20px 0 0 -0.03em; height: .8em; color: var(--ghost); white-space: nowrap; }
footer .big span { display: inline-block; transform: translateY(70%); opacity: 0; transition: transform 1.1s var(--ease) calc(var(--i) * 70ms), opacity 1.1s calc(var(--i) * 70ms), color .3s; }
footer .big.in span { transform: none; opacity: 1; }
footer .big span:hover { color: var(--red); }

@media (max-width: 900px) { .hero, .night { padding-left: 20px; padding-right: 20px; } .hero .art, .night .art { opacity: .3; } .card { position: static !important; width: auto; height: auto; margin-top: 24px; } .ground { display: none; } section, .night { margin-top: 80px; } .dock a.nav { display: none; } }
/* Lycoris at night. */
:root[data-theme="dark"] { --pearl: #141211; --plate: #1c1a18; --ink: #ece6dc; --ink-2: #a49d92; --line: #34302b; --blueprint: #e8735a; --red: #e3b768; --pale: #e8735a; --dawn-1: #141211; --dawn-2: #1f1714; --dawn-3: #2a1c16; --deep: #0b0a09; --card: #221f1c; --card-text: #ece6dc; --rule: #3a3631; --on: #7cc48e; --wait: #e0a84e; --bar: rgba(20,18,17,.92); --grid: rgba(236,230,220,.05); --ghost: #221f1c; color-scheme: dark; }
:root[data-theme="dark"] .card { box-shadow: inset 0 0 0 1px #3a3631; }
:root[data-theme="dark"] .pre { color: #8a8378; }
:root[data-theme="dark"] .night { box-shadow: inset 0 0 0 1px #2a2724; }
:root[data-theme="dark"] .night .grid { background-image: linear-gradient(rgba(236,230,220,.045) 1px, transparent 1px), linear-gradient(90deg, rgba(236,230,220,.045) 1px, transparent 1px); }
:root[data-theme="dark"] .night .kicker, :root[data-theme="dark"] .night .more, :root[data-theme="dark"] .turns span { color: #e8735a; }
:root[data-theme="dark"] .night .lede, :root[data-theme="dark"] .turns p { color: #b5ada1; }
:root[data-theme="dark"] .turns::before { background: #2f2b27; }
:root[data-theme="dark"] .turns div::before { background: #0b0a09; box-shadow: 0 0 0 1px #4a443d; }
:root[data-theme="dark"] .shelf.in:hover { box-shadow: 0 14px 30px rgba(0,0,0,.35); }
:root[data-theme="dark"] .note, :root[data-theme="dark"] .win { background: rgba(30,27,25,.97); color: #ece6dc; border-color: rgba(236,230,220,.13); }
:root[data-theme="dark"] .win { box-shadow: 0 18px 40px rgba(0,0,0,.5); }
:root[data-theme="dark"] .wm, :root[data-theme="dark"] .win .status, :root[data-theme="dark"] .win .tabs, :root[data-theme="dark"] .win .sect, :root[data-theme="dark"] .win .about { color: #a8a094; }
:root[data-theme="dark"] .win .x { color: #80796f; }
:root[data-theme="dark"] .win .tabs b { color: #ece6dc; }
:root[data-theme="dark"] .win .wbar, :root[data-theme="dark"] .win .tabs { border-color: rgba(236,230,220,.08); }
:root[data-theme="dark"] .win .sect::after { background: rgba(236,230,220,.08); }
:root[data-theme="dark"] .win .tile { background: rgba(232,115,90,.16); color: #e8735a; }
:root[data-theme="dark"] .win .act { color: #e8735a; }
:root[data-theme="dark"] .win .state, :root[data-theme="dark"] .win[data-state="2"] .status i { background: #7cc48e; }
:root[data-theme="dark"] .note i, :root[data-theme="dark"] .win .status i { background: #e0a84e; }
:root[data-theme="dark"] .win .new .state::after { color: #e0a84e; }
:root[data-theme="dark"] .win[data-state="1"] .new { background: #2e2a26; }
:root[data-theme="dark"] .win[data-state="2"] .moved { background: rgba(124,196,142,.08); }
@media (prefers-color-scheme: dark) {
:root:not([data-theme="light"]) { --pearl: #141211; --plate: #1c1a18; --ink: #ece6dc; --ink-2: #a49d92; --line: #34302b; --blueprint: #e8735a; --red: #e3b768; --pale: #e8735a; --dawn-1: #141211; --dawn-2: #1f1714; --dawn-3: #2a1c16; --deep: #0b0a09; --card: #221f1c; --card-text: #ece6dc; --rule: #3a3631; --on: #7cc48e; --wait: #e0a84e; --bar: rgba(20,18,17,.92); --grid: rgba(236,230,220,.05); --ghost: #221f1c; color-scheme: dark; }
:root:not([data-theme="light"]) .card { box-shadow: inset 0 0 0 1px #3a3631; }
:root:not([data-theme="light"]) .pre { color: #8a8378; }
:root:not([data-theme="light"]) .night { box-shadow: inset 0 0 0 1px #2a2724; }
:root:not([data-theme="light"]) .night .grid { background-image: linear-gradient(rgba(236,230,220,.045) 1px, transparent 1px), linear-gradient(90deg, rgba(236,230,220,.045) 1px, transparent 1px); }
:root:not([data-theme="light"]) .night .kicker, :root:not([data-theme="light"]) .night .more, :root:not([data-theme="light"]) .turns span { color: #e8735a; }
:root:not([data-theme="light"]) .night .lede, :root:not([data-theme="light"]) .turns p { color: #b5ada1; }
:root:not([data-theme="light"]) .turns::before { background: #2f2b27; }
:root:not([data-theme="light"]) .turns div::before { background: #0b0a09; box-shadow: 0 0 0 1px #4a443d; }
:root:not([data-theme="light"]) .shelf.in:hover { box-shadow: 0 14px 30px rgba(0,0,0,.35); }
:root:not([data-theme="light"]) .note, :root:not([data-theme="light"]) .win { background: rgba(30,27,25,.97); color: #ece6dc; border-color: rgba(236,230,220,.13); }
:root:not([data-theme="light"]) .win { box-shadow: 0 18px 40px rgba(0,0,0,.5); }
:root:not([data-theme="light"]) .wm, :root:not([data-theme="light"]) .win .status, :root:not([data-theme="light"]) .win .tabs, :root:not([data-theme="light"]) .win .sect, :root:not([data-theme="light"]) .win .about { color: #a8a094; }
:root:not([data-theme="light"]) .win .x { color: #80796f; }
:root:not([data-theme="light"]) .win .tabs b { color: #ece6dc; }
:root:not([data-theme="light"]) .win .wbar, :root:not([data-theme="light"]) .win .tabs { border-color: rgba(236,230,220,.08); }
:root:not([data-theme="light"]) .win .sect::after { background: rgba(236,230,220,.08); }
:root:not([data-theme="light"]) .win .tile { background: rgba(232,115,90,.16); color: #e8735a; }
:root:not([data-theme="light"]) .win .act { color: #e8735a; }
:root:not([data-theme="light"]) .win .state, :root:not([data-theme="light"]) .win[data-state="2"] .status i { background: #7cc48e; }
:root:not([data-theme="light"]) .note i, :root:not([data-theme="light"]) .win .status i { background: #e0a84e; }
:root:not([data-theme="light"]) .win .new .state::after { color: #e0a84e; }
:root:not([data-theme="light"]) .win[data-state="1"] .new { background: #2e2a26; }
:root:not([data-theme="light"]) .win[data-state="2"] .moved { background: rgba(124,196,142,.08); }
}
.theme { display: flex; gap: 10px; font-size: 14px; }
.theme button { font: inherit; background: none; border: 0; padding: 0; cursor: pointer; color: var(--ink-2); opacity: .7; }
.theme button:hover { opacity: 1; }
.theme button[aria-pressed="true"] { color: var(--ink); font-weight: 700; opacity: 1; }
@media (prefers-reduced-motion: reduce) { *, *::before, *::after { animation: none !important; transition: none !important; } [data-r], footer .big span { opacity: 1; transform: none; } .ink { stroke-dashoffset: 0; } .pop { stroke-opacity: 1; fill-opacity: 1; } html { scroll-behavior: auto; } h2 .w > span { transform: none; } .live { display: none; } .plate .rest { display: inline; } }
</style></head><body>
<div class="rule"></div>
<div class="dock"><div class="wrap"><a class="mark" href="#top">angelica</a><a class="nav" href="#window">In the game</a><a class="nav" href="#engine">The engine</a><a class="nav" href="#build">Build</a><a class="nav" href="#install">Install</a><a class="nav" href="#docs">Documentation</a><a class="b one" href="https://github.com/AngelicaProject/angelica/releases/download/nightly/angelica-setup.exe">Download</a></div></div>
<div class="wrap" id="top">

<nav class="bar"><a class="mark" href="#top">angelica</a><a class="nav" href="#window">In the game</a><a class="nav" href="#engine">The engine</a><a class="nav" href="#build">Build</a><a class="nav" href="#install">Install</a><a class="nav" href="#docs">Documentation</a><a class="nav" href="https://github.com/AngelicaProject/angelica">GitHub</a><span class="theme" role="group" aria-label="Theme"><button type="button" data-t="light">Light</button><button type="button" data-t="dark">Dark</button></span></nav>

<header class="hero" data-pace="1.7" data-ideas='${JSON.stringify(ideas)}'>
  <div class="grid"></div>${lily("hero", [0.184, 0.435, 0.62], [0.659, 0.263, 0.184], true)}<div class="ground"></div>
  <div class="card"><div class="cardtop"><span class="kicker">Tell your coding agent</span><button class="copy" type="button">Copy prompt</button></div>
    <p class="pre">Read angelicaproject.github.io/angelica/llms.txt, then build an Angelica plugin that</p>
    <p class="idea"><span class="field" contenteditable="plaintext-only" spellcheck="false" role="textbox" aria-label="What your plugin does"></span><i class="caret"></i></p></div>
  <div class="mid">
    <h1 class="word">angelica</h1>
    <p class="lead">An engine for every idea you bring to Eorzea.</p>
    <p class="sub">A plugin platform for FINAL FANTASY XIV. Plugins run in a sandbox and can do only what you allowed.</p>
    <div class="btns"><a class="b one" href="https://github.com/AngelicaProject/angelica/releases/download/nightly/angelica-setup.exe">Download for Windows</a><a class="b two" href="#docs">Documentation</a></div>
    <div class="status"><div class="facts">${facts.map(([label, value, detail]) => `<div><span>${label}</span><b>${value}</b><small>${detail}</small></div>`).join("")}</div></div>
  </div>
</header>

<div class="ticker" aria-hidden="true" hidden><div>${`<span>${ticker}</span>`.repeat(10)}</div></div>

<section id="window">
  <div class="split">
    <div class="head"><span class="kicker" data-r>In the game</span><h2 data-r>${rise("One small window", "over the game")}</h2>
      <p data-r style="--d:2">Press <kbd class="key">Insert</kbd> to open it. Plugins that need you are at the top, and a permission is granted only when you say so.</p></div>
    <div class="stage" data-r style="--d:1"><div class="grid"></div><div class="note"><span class="wm">angelica</span><i></i><span>1 plugin waits for you · Insert</span></div>${gameWindow}</div>
  </div>
</section>

<section id="engine">
  <div class="head"><span class="kicker" data-r>The engine</span><h2 data-r>${rise("What the engine", "takes care of")}</h2>
    <p data-r style="--d:2">Isolation, permissions, signatures, and game patches. Plugin authors do not solve these again, and players do not have to think about them.</p></div>
  <div class="features">
    ${feature("01", "Sandbox", "Nothing touches your game unasked", "Every plugin runs in a sandbox. Before it starts, it shows you what it wants to do, such as reading chat or going online, and it gets nothing beyond that. A plugin that crashes is turned off, and the game keeps running.", sandbox(), false, "sandbox")}
    ${feature("02", "Game map", "Patch day without the wait", "Plugins should keep working when the game updates. They depend on a map of the game, and Angelica brings in a new map after each patch. The plugins themselves stay as they are.", map(), true, "map")}
    ${feature("03", "Agent-first", "An agent beside every author", "A plugin can begin as a conversation with your coding agent. Angelica assumes from the start that an agent is writing beside you, so the API explains itself, the tools report what went wrong and where, and a plugin proves it works before the game is opened.", making(), false, "making")}
    ${feature("04", "Open", "Every part is written down", "Angelica is open source, and its file formats and plugin API are documented. When the API cannot do something, authors request it on GitHub, in public.", sheets(), true, "sheets")}
  </div>
</section>

<div class="night" id="build">
  <div class="grid"></div>${lily("night", [0.561, 0.718, 0.839], [0.83, 0.36, 0.26], false)}
  <span class="kicker" data-r>For builders</span>
  <h2 data-r>${rise("You describe it.", "An agent builds it.")}</h2>
  <p class="lede" data-r style="--d:2">You are the architect. Say what should happen in the game, and let your agent do the writing.</p>
  <div class="turns">
    <div><span>01</span><h3>Describe</h3><p>Write what the plugin does, in your own words. What it reads, what it changes, how you use it.</p></div>
    <div><span>02</span><h3>Build</h3><p>The agent finds the interfaces, writes the plugin, and tests it against a scripted game. No game client needed.</p></div>
    <div class="yours"><span>03</span><h3>Approve</h3><p>The plugin loads into the running game. It starts only after you approve what it asked for.</p>
      <a class="more" href="first-plugin.html">Write your first plugin →</a></div>
  </div>
</div>

<section id="install">
  <div class="head"><span class="kicker" data-r>Install</span><h2 data-r>${rise("In the game", "in three steps")}</h2></div>
  <div class="steps" data-r>
    <div data-r style="--d:1"><i>1</i><h3>Run the installer</h3><p>It finds your game folder and puts Angelica beside the game.</p></div>
    <div data-r style="--d:3"><i>2</i><h3>Start the game</h3><p>Angelica starts with it and keeps itself up to date.</p></div>
    <div data-r style="--d:5"><i>3</i><h3>Press <kbd class="key">Insert</kbd></h3><p>The Angelica window opens over the game. Pick a plugin, read what it asks for, and turn it on.</p></div>
  </div>
  <div class="after" data-r><a class="b one" href="https://github.com/AngelicaProject/angelica/releases/download/nightly/angelica-setup.exe">Download for Windows</a>
    <p>Windows 10 or later, DirectX 11 client. The installer is not code-signed, so SmartScreen may warn you the first time. Angelica checks the signature of every file it installs or loads.</p></div>
</section>

<section id="docs">
  <div class="head"><span class="kicker" data-r>Documentation</span><h2 data-r>${rise("Written for people", "and for agents")}</h2>
    <p data-r style="--d:2">If you are reading this yourself, start with a guide. If an agent is reading for you, point it at <code>llms.txt</code>, which lists every page as plain Markdown.</p></div>
  <div class="shelves">
    <div class="shelf" data-r><span class="kicker">Players</span><h3>Play</h3><ul><li><a href="install.html">Install Angelica</a></li><li><a href="turn-on-a-plugin.html">Turn on a plugin</a></li><li><a href="patch-day.html">Patch day</a></li><li><a href="remove.html">Remove Angelica</a></li><li><a href="troubleshooting.html">When something goes wrong</a></li></ul></div>
    <div class="shelf" data-r style="--d:1"><span class="kicker">Authors</span><h3>Build</h3><ul><li><a href="first-plugin.html">Write your first plugin</a></li><li><a href="run-in-the-game.html">Run your plugin in the game</a></li><li><a href="publish.html">Publish a plugin</a></li><li><a href="https://github.com/AngelicaProject/angelica/blob/main/docs/plugins/interfaces.md">Interface cards</a></li><li><a href="https://github.com/AngelicaProject/angelica/blob/main/docs/plugins/api-requests.md">Request an API</a></li></ul></div>
    <div class="shelf" data-r style="--d:2"><span class="kicker">Internals</span><h3>Understand</h3><ul><li><a href="https://github.com/AngelicaProject/angelica/blob/main/docs/architecture/lens.md">How plugins see the game</a></li><li><a href="https://github.com/AngelicaProject/angelica/blob/main/docs/architecture/game-map.md">The game map</a></li><li><a href="https://github.com/AngelicaProject/angelica/blob/main/docs/architecture/permissions.md">The sandbox and permissions</a></li><li><a href="https://github.com/AngelicaProject/angelica/blob/main/docs/architecture/loading.md">How Angelica loads into the game</a></li><li><a href="https://github.com/AngelicaProject/angelica/blob/main/docs/development/README.md">Contribute</a></li></ul></div>
  </div>
</section>

<footer><div class="row"><a href="https://github.com/AngelicaProject/angelica">Source code</a><a href="https://github.com/AngelicaProject/angelica/releases">Releases</a><span>Angelica is not affiliated with Square Enix. FINAL FANTASY XIV is a trademark of Square Enix Holdings Co., Ltd.</span></div>
  <div class="big" data-r aria-hidden="true">${letters("angelica")}</div></footer>

</div>
<script>
const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
// The theme: the reader's last choice, otherwise the system's. At night the hero's lily is printed in brighter inks.
const inks = { light: { line: [0.184, 0.435, 0.62], fill: [0.659, 0.263, 0.184] }, dark: { line: [0.79, 0.75, 0.68], fill: [0.91, 0.42, 0.33] } };
const themeButtons = [...document.querySelectorAll(".theme button")];
function theme(t, keep) {
  document.documentElement.dataset.theme = t;
  themeButtons.forEach((b) => b.setAttribute("aria-pressed", b.dataset.t === t));
  for (const part of ["line", "fill"]) { const funcs = document.querySelectorAll(["R", "G", "B"].map((c) => "#" + part + "-hero feFunc" + c).join(", ")); funcs.forEach((f, i) => f.setAttribute("intercept", inks[t][part][i])); }
  if (keep) try { localStorage.setItem("theme", t); } catch {}
}
let savedTheme = null; try { savedTheme = localStorage.getItem("theme"); } catch {}
theme(savedTheme || (matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light"));
themeButtons.forEach((b) => b.addEventListener("click", () => theme(b.dataset.t, true)));
const clamp = (v) => Math.max(0, Math.min(1, v));

// A lily has two thresholds: how much of its line drawing exists, and how much of that has turned red. Each eases toward a target every frame.
const reach = (t) => (-18 * (1 - 0.8784 * t)).toFixed(3);
function flower(art, pace) {
  const line = art.querySelector('[id^="line-"] feFuncA'), fill = art.querySelector('[id^="fill-"] feFuncA');
  const now = { line: 0, fill: 0 }, want = { line: 0, fill: 0 }, shown = { line: "", fill: "" };
  let visible = true;
  new IntersectionObserver(([entry]) => { visible = entry.isIntersecting; }, { rootMargin: "120px" }).observe(art);
  // A filter is rewritten only when its value has really changed; rewriting one repaints the whole flower.
  const set = (node, key, value) => { const text = reach(value); if (text !== shown[key]) { shown[key] = text; node.setAttribute("intercept", text); } };
  const frame = () => {
    requestAnimationFrame(frame);
    if (!visible) return;
    now.line += Math.max(-0.035 / pace, Math.min(0.0065 / pace, want.line - now.line));
    now.fill = Math.abs(want.fill - now.fill) < 0.002 ? want.fill : now.fill + (want.fill - now.fill) * (0.12 / pace);
    set(line, "line", still ? 1 : Math.max(0, now.line));
    set(fill, "fill", still ? 1 : now.fill);
  };
  requestAnimationFrame(frame);
  return want;
}

// Hero: the sentence in the card drives the lily. It types examples until the visitor writes their own.
{
  const PREFIX = "Read https://angelicaproject.github.io/angelica/llms.txt, then build an Angelica plugin that ";
  const hero = document.querySelector(".hero"), field = hero.querySelector(".field"), copy = hero.querySelector(".copy");
  const ideas = JSON.parse(hero.dataset.ideas), pace = Number(hero.dataset.pace), want = flower(hero.querySelector(".art"), pace);
  let idea = 0, at = 0, erasing = false, timer = 0, own = false;
  const grow = () => { const g = clamp(field.textContent.trim().length / (own ? 34 : ideas[idea].length)); hero.style.setProperty("--grown", g.toFixed(3)); want.fill = g; want.line = g > 0 ? Math.min(1, 0.16 + g * 1.5) : 0; };
  const demo = () => {
    const text = ideas[idea];
    if (!erasing) {
      field.textContent = text.slice(0, ++at); grow();
      if (at === text.length) { erasing = true; return (timer = setTimeout(demo, 3800 + 1400 * (pace - 1))); }
      return (timer = setTimeout(demo, (36 + Math.random() * 44) * pace));
    }
    field.textContent = text.slice(0, (at = Math.max(0, at - 1))); grow();
    if (at === 0) { erasing = false; idea = (idea + 1) % ideas.length; return (timer = setTimeout(demo, 900)); }
    timer = setTimeout(demo, 18 * pace);
  };
  if (still) { field.textContent = ideas[0]; hero.style.setProperty("--grown", 1); } else timer = setTimeout(demo, 1100);
  field.addEventListener("focus", () => { if (own) return; clearTimeout(timer); own = true; field.textContent = ""; hero.classList.add("typing"); grow(); });
  field.addEventListener("input", grow);
  field.addEventListener("keydown", (e) => { if (e.key === "Enter") { e.preventDefault(); copy.click(); } });
  field.addEventListener("blur", () => { if (field.textContent.trim() || still) return; own = false; hero.classList.remove("typing"); at = 0; erasing = false; timer = setTimeout(demo, 600); });
  copy.addEventListener("click", () => {
    const said = field.textContent.trim().replace(/\\.$/, "") || ideas[idea];
    navigator.clipboard?.writeText(PREFIX + said + ".").catch(() => {});
    copy.textContent = "Copied"; copy.classList.add("done");
    setTimeout(() => { copy.textContent = "Copy prompt"; copy.classList.remove("done"); }, 1600);
  });
}

// Things arrive, and plates are drawn, when they come into view.
const seen = new IntersectionObserver((entries) => { for (const e of entries) if (e.isIntersecting) { e.target.classList.add("in"); seen.unobserve(e.target); } }, { threshold: 0.18, rootMargin: "0px 0px -6% 0px" });
for (const el of document.querySelectorAll("[data-r]")) still ? el.classList.add("in") : seen.observe(el);

// What follows the scroll itself: the page rule, the small bar, the ticker, the plates' slight slide, and the night band, whose lily is drawn as it passes.
const night = document.querySelector(".night"), nightWant = flower(night.querySelector(".art"), 0.6), turns = [...night.querySelectorAll(".turns div")];
const dock = document.querySelector(".dock"), rule = document.querySelector(".rule"), heroEl = document.querySelector(".hero"), run = document.querySelector(".ticker div");
let asked = false;
const follow = () => {
  asked = false;
  const y = scrollY, vh = innerHeight, full = document.documentElement.scrollHeight - vh;
  rule.style.transform = "scaleX(" + clamp(y / full).toFixed(4) + ")";
  dock.classList.toggle("on", heroEl.getBoundingClientRect().bottom < 60);
  // The band's story is told by the time it sits in the middle of the window (or reaches the top, when it is taller than the window).
  const box = night.getBoundingClientRect(), rest = Math.max(70, (vh - box.height) / 2), from = vh * 0.92, p = clamp((from - box.top) / (from - rest));
  if (box.bottom > -200 && box.top < vh + 200) night.style.setProperty("--p", p.toFixed(3));
  nightWant.line = clamp(p * 2.1); nightWant.fill = clamp((p - 0.38) * 1.7);
  turns.forEach((turn, i) => turn.classList.toggle("on", p > 0.14 + i * 0.3));
};
// The ticker never stops; scrolling only pushes it along.
if (!still && !run.parentElement.hidden) { let lap = run.firstElementChild.getBoundingClientRect().width; addEventListener("resize", () => { lap = run.firstElementChild.getBoundingClientRect().width; }); const drift = (t) => { run.style.transform = "translate3d(" + (-((scrollY * 0.25 + t * 0.025) % lap)).toFixed(2) + "px, 0, 0)"; requestAnimationFrame(drift); }; requestAnimationFrame(drift); }
// A plate leans a little toward the pointer.
for (const plate of document.querySelectorAll(".plate")) {
  const picture = plate.querySelector(".plate-art");
  plate.addEventListener("pointermove", (e) => { const b = plate.getBoundingClientRect(); picture.style.setProperty("--ry", (((e.clientX - b.left) / b.width - 0.5) * 7).toFixed(2) + "deg"); picture.style.setProperty("--rx", (((e.clientY - b.top) / b.height - 0.5) * -6).toFixed(2) + "deg"); });
  plate.addEventListener("pointerleave", () => { picture.style.setProperty("--ry", "0deg"); picture.style.setProperty("--rx", "0deg"); });
}
// The window: a plugin waits, the player allows it, it runs.
for (const win of document.querySelectorAll(".win")) {
  if (still) break;
  const waits = [2800, 2000, 3600]; let state = 0;
  const next = () => { state = (state + 1) % 3; win.dataset.state = state; win.querySelector(".count").textContent = state === 2 ? "2" : "1"; win.parentElement.classList.toggle("calm", state === 2); setTimeout(next, waits[state]); };
  setTimeout(next, waits[0]);
}
addEventListener("scroll", () => { if (!asked) { asked = true; requestAnimationFrame(follow); } }, { passive: true });
addEventListener("resize", follow);
follow();
</script></body></html>`;
writeFileSync(out, html);
console.log("wrote", html.length);
