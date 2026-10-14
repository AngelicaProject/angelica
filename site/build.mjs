// Builds the website into dist/: the home page, a page for every Markdown file in pages/,
// the same pages as Markdown, and llms.txt. Then checks that every link inside the site
// leads to a file and, with a #, to an element of that file.
// Usage: node build.mjs
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { page, read } from "./docs.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const dist = join(here, "dist");
const site = "https://angelicaproject.github.io/angelica/";
const raw = "https://raw.githubusercontent.com/AngelicaProject/angelica/main/";

rmSync(dist, { recursive: true, force: true });
mkdirSync(dist);
execFileSync(process.execPath, [join(here, "home.mjs"), join(here, "art"), join(dist, "index.html")], { stdio: "inherit" });

const sources = readdirSync(join(here, "pages")).filter((f) => f.endsWith(".md")).sort();
const docs = sources.map((f) => ({ file: f.replace(/\.md$/, ".html"), ...read(readFileSync(join(here, "pages", f), "utf8")) }))
  .sort((a, b) => (Number(a.front.order) || 99) - (Number(b.front.order) || 99));
const pages = Object.fromEntries(docs.map((d) => [d.title.replace(/^[^:]+: /, ""), d.file]));
for (const d of docs) {
  writeFileSync(join(dist, d.file), page(d, d.file, pages));
  writeFileSync(join(dist, d.file.replace(/\.html$/, ".md")), d.md.trimStart());
}

// llms.txt: what an agent reads first. Every page as Markdown, then the canonical documents an author needs.
const lede = (md) => md.replace(/^# .+$/m, "").trim().split(/\r?\n\r?\n/)[0].replace(/\s+/g, " ").replace(/\[([^\]]+)\]\([^)]+\)/g, "$1");
const listed = (section) => docs.filter((d) => (d.front.section || "Build") === section).map((d) => `- [${d.title.replace(/^[^:]+: /, "")}](${site}${d.file.replace(/\.html$/, ".md")}): ${lede(d.md)}`);
const canonical = [
  ["Writing a plugin", "docs/plugins/authoring.md", "The steps from an empty folder to a published plugin, with the commands."],
  ["Interface cards", "docs/plugins/interfaces.md", "What each part of the plugin API does, its permission, and how to use it."],
  ["Plugin API 1.0", "docs/formats/plugin-api-v1.md", "The contract between a plugin and the host."],
  ["Plugin manifest", "docs/formats/plugin-manifest-v1.md", "The fields of angelica.toml."],
  ["Permissions", "docs/architecture/permissions.md", "Every permission, what it allows, and how the player sees it."],
  ["API requests", "docs/plugins/api-requests.md", "How to ask for an operation the API does not have."],
];
writeFileSync(join(dist, "llms.txt"), `# Angelica

> A plugin platform for FINAL FANTASY XIV. A plugin is a Rust WebAssembly component that runs in a sandbox and can do only what the player allowed. It names entries of the game map instead of game addresses, so it keeps working across game patches.

## Players

${listed("Play").join("\n")}

## Plugin authors

${listed("Build").join("\n")}
${canonical.map(([t, path, about]) => `- [${t}](${raw}${path}): ${about}`).join("\n")}

## Optional

- [Architecture overview](${raw}docs/architecture/overview.md): The components of Angelica and what each owns.
- [Product principles](${raw}docs/product/principles.md): The invariants every change keeps.
`);

// Every link that stays inside the site must lead somewhere.
const broken = [];
for (const file of readdirSync(dist).filter((f) => f.endsWith(".html"))) {
  const html = readFileSync(join(dist, file), "utf8");
  for (const [, href] of html.matchAll(/\s(?:href|src)="([^"]*)"/g)) {
    if (/^(?:[a-z]+:|\/\/|data:)/i.test(href)) continue;
    const [target, hash] = href.split("#");
    const into = target || file;
    let text;
    try { text = readFileSync(join(dist, into), "utf8"); } catch { broken.push(`${file}: ${href}`); continue; }
    if (hash && into.endsWith(".html") && !new RegExp(`\\sid="${hash.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}"`).test(text)) broken.push(`${file}: ${href}`);
  }
}
if (broken.length) {
  console.error(`Links that lead nowhere:\n${broken.join("\n")}`);
  process.exit(1);
}
console.log(`built ${docs.length + 1} pages into dist/`);
