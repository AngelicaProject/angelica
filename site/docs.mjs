// Documentation pages in the home page's look, each built from one Markdown file in pages/.
// A page starts with a few facts between --- lines: its section, the word above its title,
// the facts under the title, and the titles of the pages before and after it.
import { Marked } from "marked";

const repo = "https://github.com/AngelicaProject/angelica";
const esc = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
const slug = (s) => s.toLowerCase().replace(/<[^>]+>/g, "").replace(/&[a-z]+;/g, "").replace(/[^a-z0-9 -]/g, "").trim().replace(/ +/g, "-");

// Colour code the way a reader scans it: names of the language quiet, the parts you change loud.
const rules = {
  rust: [[/\/\/.*$/m, "c"], [/"(?:\\.|[^"\\])*"/, "s"], [/#\[[^\]]*\]/, "a"], [/\b(?:fn|let|if|else|match|struct|impl|use|return|mut|pub|for|in|enum|mod|const|crate)\b/, "k"], [/\b(?:self|Self|Ok|Err|Some|None|true|false)\b/, "v"], [/\b[A-Z][A-Za-z0-9]*\b/, "t"], [/\b\d+\b/, "n"]],
  toml: [[/#.*$/m, "c"], [/^\s*\[[^\]]*\]/m, "k"], [/"(?:\\.|[^"\\])*"/, "s"], [/\b(?:true|false)\b/, "v"], [/\b\d+\b/, "n"]],
  shell: [[/#.*$/m, "c"], [/^\s*[a-z][\w-]*/m, "k"]],
};
function paint(code, lang) {
  const set = rules[lang];
  if (!set) return esc(code);
  const all = new RegExp(set.map(([r]) => `(${r.source})`).join("|"), "gm");
  return code.split("\n").map((line) => {
    let html = "", at = 0;
    for (const m of line.matchAll(all)) {
      const kind = set[m.slice(1).findIndex((g) => g !== undefined)][1];
      html += esc(line.slice(at, m.index)) + `<span class="${kind}">${esc(m[0])}</span>`;
      at = m.index + m[0].length;
    }
    return html + esc(line.slice(at));
  }).join("\n");
}

/** The facts at the top of a page and the Markdown after them. */
export function read(text) {
  const front = {};
  const md = text.replace(/^---\r?\n([\s\S]*?)\r?\n---\r?\n/, (_, block) => {
    for (const line of block.split(/\r?\n/)) { const at = line.indexOf(":"); if (at > 0) front[line.slice(0, at).trim()] = line.slice(at + 1).trim(); }
    return "";
  });
  const title = md.match(/^# (.+)$/m)?.[1];
  if (!title) throw new Error("a page needs a # title");
  return { front, md, title };
}

// The menu follows the home page's shelves. A name is a page from pages/ by its title; a pair is a link elsewhere.
const doc = (path) => `${repo}/blob/main/docs/${path}`;
const nav = [
  ["Play", ["Install Angelica", "Turn on a plugin", "Patch day", "Remove Angelica", "When something goes wrong"]],
  ["Build", ["Build a plugin with an agent", "Write your first plugin", "Run your plugin in the game", "Publish a plugin", ["Writing a plugin", doc("plugins/authoring.md")], ["Interface cards", doc("plugins/interfaces.md")], ["Plugin API 1.0", doc("formats/plugin-api-v1.md")], ["Plugin manifest", doc("formats/plugin-manifest-v1.md")], ["Request an API", doc("plugins/api-requests.md")]]],
  ["Understand", [["Overview", doc("architecture/overview.md")], ["How plugins see the game", doc("architecture/lens.md")], ["Loading into the game", doc("architecture/loading.md")], ["The game map", doc("architecture/game-map.md")], ["Permissions", doc("architecture/permissions.md")], ["Contribute", doc("development/README.md")]]],
];

/** One page as HTML. `pages` maps every page title to its file name. */
export function page({ front, md, title }, file, pages) {
  const headings = [];
  const renderer = {
    heading(text, level) {
      const id = slug(text);
      if (level === 2) headings.push({ id, text: text.replace(/<[^>]+>/g, "") });
      if (level === 1) return "";
      return `<h${level} id="${id}"><a class="anchor" href="#${id}" aria-label="Link to this section">#</a>${text}</h${level}>`;
    },
    code(code, info) {
      const lang = (info || "").trim();
      if (lang === "plaintext") return `<figure class="output"><figcaption>Output</figcaption><pre><code>${esc(code)}</code></pre></figure>`;
      const label = { shell: "Terminal", rust: "Rust", toml: "TOML" }[lang] || lang;
      return `<figure class="code"><figcaption><span>${label}</span><button type="button" class="copy">Copy</button></figcaption><pre><code>${paint(code, lang)}</code></pre></figure>`;
    },
    table(header, body) { return `<div class="table"><table><thead>${header}</thead><tbody>${body}</tbody></table></div>`; },
  };
  // Links between pages are written to the Markdown files and served as HTML.
  const body = new Marked({ renderer, gfm: true }).parse(md).replace(/href="([^":#]+)\.md(#[^"]*)?"/g, (_, name, hash = "") => `href="${name}.html${hash}"`);
  const lede = body.match(/<p>([\s\S]*?)<\/p>/)[1];
  const rest = body.replace(/<p>[\s\S]*?<\/p>/, "");
  const [kind, name] = title.includes(": ") ? title.split(": ") : [front.kicker || "Guide", title];

  const item = (entry) => {
    if (Array.isArray(entry)) return `<li><a href="${entry[1]}">${entry[0]}</a></li>`;
    if (!pages[entry]) return "";
    return `<li><a href="${pages[entry]}"${entry === title ? ' aria-current="page"' : ""}>${entry}</a></li>`;
  };
  const sidebar = `<nav class="side" aria-label="Documentation">${nav.map(([group, items]) => `<p class="kicker">${group}</p><ul>${items.map(item).join("")}</ul>`).join("")}</nav>`;
  const source = `site/pages/${file.replace(/\.html$/, ".md")}`;
  const toc = `<aside class="toc">${headings.length ? `<p class="kicker">On this page</p>` : ""}<ul>${headings.map((h) => `<li><a href="#${h.id}">${h.text}</a></li>`).join("")}</ul><p class="edit"><a href="${repo}/edit/main/${source}">Edit this page</a></p></aside>`;
  const top = `<header class="bar"><div class="in"><a class="mark" href="index.html">angelica</a><a class="where" href="index.html#docs">Documentation</a>
  <span class="theme" role="group" aria-label="Theme"><button type="button" data-t="light">Light</button><button type="button" data-t="dark">Dark</button></span><a class="nav" href="${repo}">GitHub</a><a class="b one" href="${repo}/releases/download/nightly/angelica-setup.exe">Download</a></div></header>`;
  const meta = front.meta ? `<p class="meta">${front.meta.split(" · ").map((m) => `<span>${m}</span>`).join("")}</p>` : "";
  const crumbs = `<p class="crumbs"><a href="index.html#docs">Documentation</a> / <span>${front.section || "Build"}</span> / <span>${name}</span></p>`;
  const near = (t, label, cls) => t && pages[t] ? `<a href="${pages[t]}"${cls}><small>${label}</small>${t}</a>` : "<span></span>";
  const foot = `<div class="pager">${near(front.prev, "Previous", "")}${near(front.next, "Next", ' class="next"')}</div>
  <footer class="foot"><span>This page is <code>${source}</code> in the Angelica repository.</span><a href="${repo}/issues/new">Something here is wrong</a></footer>`;
  const head = `<p class="kicker">${kind}</p><h1>${name}</h1><p class="lede">${lede}</p>`;

  return `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>${name}: Angelica</title>
<link rel="alternate" type="text/markdown" href="${file.replace(/\.html$/, ".md")}">
<link rel="preconnect" href="https://fonts.googleapis.com"><link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Cormorant+Garamond:ital,wght@0,300;0,400;1,300;1,400&family=JetBrains+Mono:wght@400;500;600;700&family=Manrope:wght@400;500;600;700&display=swap">
<style>${css}</style></head><body>
${top}<div class="page">${sidebar}<main class="doc">${crumbs}${head}${meta}${rest}${foot}</main>${toc}</div>
<script>${js}</script></body></html>`;
}

// Lycoris at night: warm black, the red as the colour of links and marks.
const night = "--pearl: #141211; --plate: #1c1a18; --ink: #ece6dc; --ink-2: #a49d92; --line: #2a2724; --line-2: #3a3631; --blueprint: #e8735a; --red: #e3b768; --bar: rgba(20,18,17,.92); --tint: rgba(232,115,90,.12); --on: #7cc48e; --c-k: #e8735a; --c-v: #d8a8c8; --c-s: #e3b768; --c-t: #c9bfae; --c-n: #9fc6e0;";
const dark = `:root[data-theme="dark"] { ${night} color-scheme: dark; }
@media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) { ${night} color-scheme: dark; } }`;

const css = `
:root { --pearl: #f4f2ee; --plate: #fbfaf8; --ink: #16202f; --ink-2: #5d6676; --line: #d6d8dc; --line-2: #c9cdd4; --blueprint: #3f7fa8; --red: #a8432f; --gold: #e3b768; --dawn-1: #dfe7f1; --dawn-2: #f3dccb; --dawn-3: #f7ecd9;
  --soft: "Cormorant Garamond", "Times New Roman", serif; --sans: "Manrope", "Segoe UI", system-ui, sans-serif; --mono: "JetBrains Mono", Consolas, ui-monospace, monospace; --ease: cubic-bezier(.2, .7, .2, 1);
  --bar: rgba(244,242,238,.94); --tint: rgba(63,127,168,.09); --on: #2e7d46; --c-k: #3f7fa8; --c-v: #7a4fa0; --c-s: #a8432f; --c-t: #1f5d82; --c-n: #9a6a12; color-scheme: light; }
${dark}
* { box-sizing: border-box; }
html { scroll-behavior: smooth; scroll-padding-top: 84px; }
body { margin: 0; background: var(--pearl); color: var(--ink); font-family: var(--sans); font-size: 17px; line-height: 1.65; }
a { color: inherit; }
.kicker { font-family: var(--mono); font-size: 11.5px; letter-spacing: .14em; text-transform: uppercase; color: var(--blueprint); margin: 0; }
.b { font-family: var(--mono); font-size: 12px; font-weight: 700; letter-spacing: .08em; text-transform: uppercase; padding: 9px 14px; border-radius: 2px; text-decoration: none; transition: background .15s, transform .15s; }
.b.one { background: var(--ink); color: var(--pearl); }
.b.one:hover { background: var(--red); transform: translateY(-1px); }

.bar { position: sticky; top: 0; z-index: 10; background: var(--bar); backdrop-filter: blur(6px); border-bottom: 1px solid var(--line); }
.bar .in { max-width: 1360px; margin: 0 auto; padding: 10px 24px; display: flex; align-items: center; gap: 22px; }
.mark { font-family: var(--soft); font-style: italic; font-weight: 300; font-size: 27px; line-height: 1; text-decoration: none; }
.where { font-size: 14px; font-weight: 600; color: var(--ink-2); padding-left: 18px; border-left: 1px solid var(--line-2); margin-right: auto; text-decoration: none; }
.bar a.nav { font-size: 14px; font-weight: 600; text-decoration: none; color: var(--ink-2); }
.bar a.nav:hover { color: var(--red); }
.theme { display: flex; gap: 10px; font-size: 13.5px; }
.theme button { font: inherit; background: none; border: 0; padding: 0; cursor: pointer; color: var(--ink-2); opacity: .7; }
.theme button:hover { opacity: 1; }
.theme button[aria-pressed="true"] { color: var(--ink); font-weight: 700; opacity: 1; }

.page { max-width: 1360px; margin: 0 auto; padding: 0 24px; display: grid; grid-template-columns: 236px minmax(0, 1fr) 216px; gap: 56px; }
.side, .toc { position: sticky; top: 76px; align-self: start; max-height: calc(100vh - 92px); overflow: auto; padding: 36px 0 24px; }
.side .kicker { margin: 22px 0 6px; }
.side .kicker:first-child { margin-top: 0; }
.side ul, .toc ul { list-style: none; margin: 0; padding: 0; }
.side a { display: block; padding: 4px 0 4px 12px; border-left: 2px solid transparent; text-decoration: none; font-size: 15px; color: var(--ink-2); transition: color .15s, border-color .15s; }
.side a:hover { color: var(--ink); border-left-color: var(--line-2); }
.side .part { font-size: 12.5px; font-weight: 600; color: var(--ink); padding: 12px 0 2px 14px; }
kbd { font: 600 .8em var(--mono); border: 1px solid var(--line-2); border-bottom-width: 2px; border-radius: 3px; padding: 0 6px; background: var(--plate); }
.side a[aria-current] { color: var(--ink); font-weight: 600; border-left-color: var(--red); }

.toc { font-size: 14px; }
.toc .kicker { margin-bottom: 10px; }
.toc a { display: block; padding: 3px 0 3px 12px; border-left: 1px solid var(--line); text-decoration: none; color: var(--ink-2); transition: color .2s, border-color .2s; }
.toc a:hover { color: var(--ink); }
.toc a.here { color: var(--ink); border-left: 2px solid var(--red); padding-left: 11px; }
.toc .edit { margin: 18px 0 0; padding-left: 12px; }
.toc .edit a { border: 0; padding: 0; color: var(--blueprint); }

.doc { max-width: 720px; padding: 36px 0 0; }
.crumbs { font-size: 13.5px; color: var(--ink-2); margin: 0 0 22px; }
.crumbs a { text-decoration: none; }
.crumbs a:hover { color: var(--red); }
h1 { font-family: var(--soft); font-weight: 300; font-size: clamp(44px, 5vw, 64px); line-height: 1.02; letter-spacing: -.01em; margin: 8px 0 18px; text-wrap: balance; }
.lede { font-size: 20px; line-height: 1.55; color: var(--ink); margin: 0 0 16px; max-width: 36em; }
.meta { display: flex; flex-wrap: wrap; gap: 6px; margin: 0 0 8px; }
.meta span { font-family: var(--mono); font-size: 11.5px; letter-spacing: .04em; color: var(--ink-2); border: 1px solid var(--line-2); border-radius: 2px; padding: 3px 9px; }

.doc h2 { font-size: 27px; font-weight: 700; letter-spacing: -.01em; line-height: 1.25; margin: 64px 0 14px; position: relative; }
.doc h3 { font-size: 19px; font-weight: 700; margin: 36px 0 8px; position: relative; }
.anchor { position: absolute; left: -1.1em; width: 1em; color: var(--blueprint); text-decoration: none; opacity: 0; transition: opacity .15s; font-weight: 500; }
h2:hover .anchor, h3:hover .anchor { opacity: 1; }
.doc p { margin: 0 0 16px; }
.doc ul, .doc ol { padding-left: 22px; margin: 0 0 18px; }
.doc li { margin: 4px 0; }
.doc li::marker { color: var(--ink-2); }
.doc a { color: var(--blueprint); text-decoration-thickness: 1px; text-underline-offset: 3px; }
.doc a:hover { color: var(--red); }
.doc :not(pre) > code { font-family: var(--mono); font-size: .84em; background: var(--tint); border-radius: 3px; padding: 1px 5px; }

/* Numbered steps: the number stands in the margin, the way the install steps do on the home page. */
.doc ol { counter-reset: step; list-style: none; padding-left: 44px; }
.doc ol > li { counter-increment: step; position: relative; margin: 0 0 18px; }
.doc ol > li::before { content: counter(step); position: absolute; left: -44px; top: -6px; width: 30px; text-align: right; font-family: var(--soft); font-style: italic; font-size: 30px; line-height: 1.2; color: var(--blueprint); }

figure { margin: 14px 0 20px; }
.code { background: var(--plate); border: 1px solid var(--line-2); border-radius: 4px; overflow: hidden; }
.code figcaption { display: flex; justify-content: space-between; align-items: center; padding: 6px 8px 6px 14px; border-bottom: 1px solid var(--line); font-family: var(--mono); font-size: 11px; letter-spacing: .12em; text-transform: uppercase; color: var(--ink-2); }
.copy { font: 600 11px var(--mono); letter-spacing: .08em; text-transform: uppercase; color: var(--blueprint); background: none; border: 0; padding: 3px 6px; border-radius: 2px; cursor: pointer; }
.copy:hover { background: var(--tint); }
.copy.done { color: var(--on); }
pre { margin: 0; padding: 14px 16px; overflow-x: auto; font-family: var(--mono); font-size: 13.5px; line-height: 1.6; }
.code .k { color: var(--c-k); font-weight: 500; }
.code .v { color: var(--c-v); }
.code .s { color: var(--c-s); }
.code .t { color: var(--c-t); }
.code .n { color: var(--c-n); }
.code .a { color: var(--c-n); }
.code .c { color: var(--ink-2); font-style: italic; }
/* What the computer answers: no box, a rule on the left, so it never looks like something to type. */
.output { border-left: 2px solid var(--line-2); padding-left: 2px; }
.output figcaption { font-family: var(--mono); font-size: 11px; letter-spacing: .12em; text-transform: uppercase; color: var(--ink-2); padding: 0 14px; }
.output pre { color: var(--ink-2); padding-top: 4px; padding-bottom: 4px; }

.table { overflow-x: auto; margin: 14px 0 22px; }
table { border-collapse: collapse; width: 100%; font-size: 15.5px; }
th { text-align: left; font-family: var(--mono); font-size: 11px; letter-spacing: .12em; text-transform: uppercase; color: var(--ink-2); font-weight: 500; padding: 0 16px 8px 0; border-bottom: 1px solid var(--ink); }
td { padding: 10px 16px 10px 0; border-bottom: 1px solid var(--line); vertical-align: top; }
td:first-child { white-space: nowrap; }

.pager { display: grid; grid-template-columns: 1fr 1fr; gap: 14px; margin: 72px 0 0; }
.pager a { border: 1px solid var(--line-2); border-radius: 4px; padding: 14px 18px; text-decoration: none; font-weight: 600; color: var(--ink); background: var(--plate); transition: border-color .2s, transform .2s var(--ease); }
.pager a:hover { border-color: var(--blueprint); transform: translateY(-2px); color: var(--ink); }
.pager small { display: block; font-family: var(--mono); font-size: 11px; letter-spacing: .12em; text-transform: uppercase; color: var(--ink-2); font-weight: 400; }
.pager .next { text-align: right; }
.foot { display: flex; justify-content: space-between; gap: 12px 24px; flex-wrap: wrap; margin: 40px 0 64px; padding-top: 18px; border-top: 1px solid var(--line); font-size: 13.5px; color: var(--ink-2); }
.foot a { color: var(--blueprint); }

@media (max-width: 1180px) { .page { grid-template-columns: 220px minmax(0, 1fr); } .toc { display: none; } }
@media (max-width: 860px) { .page { grid-template-columns: minmax(0, 1fr); padding: 0 16px; } .side { display: none; } .where { display: none; } .bar .in { padding: 10px 16px; } .bar .mark { margin-right: auto; } }
@media (prefers-reduced-motion: reduce) { * { animation: none !important; transition: none !important; } html { scroll-behavior: auto; } }
`;

const js = `
// The theme: the reader's last choice, otherwise the system's.
const root = document.documentElement, buttons = [...document.querySelectorAll(".theme button")];
const system = () => matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
function theme(t, keep) { root.dataset.theme = t; buttons.forEach((b) => b.setAttribute("aria-pressed", b.dataset.t === t)); if (keep) try { localStorage.setItem("theme", t); } catch {} }
let saved = null; try { saved = localStorage.getItem("theme"); } catch {}
theme(saved || system());
buttons.forEach((b) => b.addEventListener("click", () => theme(b.dataset.t, true)));
document.querySelectorAll(".copy").forEach((b) => b.addEventListener("click", async () => {
  try { await navigator.clipboard.writeText(b.closest("figure").querySelector("code").innerText); b.textContent = "Copied"; b.classList.add("done"); setTimeout(() => { b.textContent = "Copy"; b.classList.remove("done"); }, 1600); } catch {}
}));
// Which section is being read: the last heading that has passed the top third of the window.
const hs = [...document.querySelectorAll(".doc h2")], links = (sel) => [...document.querySelectorAll(sel)];
const toc = links(".toc a[href^='#']");
let queued = false;
function follow() {
  queued = false;
  const mark = innerHeight * .33; let at = -1;
  hs.forEach((h, i) => { if (h.getBoundingClientRect().top < mark) at = i; });
  const id = at >= 0 ? hs[at].id : null;
  toc.forEach((a) => a.classList.toggle("here", a.hash === "#" + id));
}
addEventListener("scroll", () => { if (!queued) { queued = true; requestAnimationFrame(follow); } }, { passive: true });
follow();
`;
