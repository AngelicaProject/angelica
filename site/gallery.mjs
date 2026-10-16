// The Interface parts page: every place and part a plugin builds, drawn by Angelica's own renderer, with the
// code that builds it, the API version that added it, and its properties. The pictures and gallery.json come
// from the host's gallery test (crates/angelica-host/src/interop/gallery.rs), which runs on Windows;
// docs/development/website.md says how. The versions come from the plugin API's list of kinds in angelica-view.
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

// The groups of the plugin API's tables of kinds, in their order. `end` closes a container and has no entry.
const groups = [
  ["Places", ["window", "dialog", "panel", "popup"]],
  ["Layout", ["row", "column", "grid", "section", "table", "spacer", "separator", "tabs"]],
  ["Text", ["text", "heading", "fact", "tooltip", "tile", "icon"]],
  ["Values", ["meter", "ring", "readout", "countdown", "sparkline"]],
  ["Actions and input", ["button", "checkbox", "text-field", "choice", "number", "entry"]],
];

const esc = (text) => text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/"/g, "&quot;");
const minor = (version) => Number(version.split(".")[1]);

/** The page as Markdown, or null when the gallery has not been drawn into `dir`. */
export function gallery(dir) {
  const index = join(dir, "gallery.json");
  if (!existsSync(index)) return null;
  const { api, kinds, examples } = JSON.parse(readFileSync(index, "utf8"));
  const byKind = new Map(kinds.map((k) => [k.kind, k]));
  const listed = groups.flatMap(([, names]) => names);
  const unlisted = kinds.filter((k) => k.kind !== "end" && !listed.includes(k.kind));
  if (unlisted.length) throw new Error(`gallery.mjs: no group for ${unlisted.map((k) => k.kind).join(", ")}`);
  const sections = groups.map(([title, names]) => [`## ${title}`, ...names.map((name) => kind(byKind.get(name), examples.filter((e) => e.kind === name)))].join("\n\n"));
  return [
    "# Interface parts",
    "Every place and part a plugin can show, as Angelica draws it in the game. The code under each picture builds it; the [plugin API](reference-plugin-api.html#places-and-parts) gives each property's values and limits.",
    `API ${api} is the newest. A plugin that uses a kind or a property of a later version than the one in its manifest's \`plugin.api\` raises \`plugin.api\` to that version, and a host that runs only an older version refuses the plugin.`,
    versions(kinds, api),
    contents(byKind),
    ...sections,
  ].join("\n\n") + "\n";
}

/** What each version of API 1 added. */
function versions(kinds, api) {
  const rows = [];
  for (let m = 0; m <= minor(api); m++) {
    const version = `1.${m}`;
    if (m === 0) {
      rows.push(`| ${version} | The first version: every kind and property on this page unless a later row names it. |`);
      continue;
    }
    const added = [
      ...kinds.filter((k) => k.since === version).map((k) => `[\`${k.kind}\`](#${k.kind})`),
      ...kinds.flatMap((k) => (k.since === version ? [] : k.properties.filter((p) => p.since === version).map((p) => `[\`${k.kind}\`](#${k.kind}) \`${p.name}\``))),
    ];
    rows.push(`| ${version} | ${added.length ? added.join(", ") : "Nothing for places and parts."} |`);
  }
  return ["## By version", "| Version | Added |\n|---|---|\n" + rows.join("\n")].join("\n\n");
}

/** Every kind by its group, with the version that added it. */
function contents(byKind) {
  const rows = groups.map(([title, names]) => `| ${title} | ${names.map((name) => `[\`${name}\`](#${name})${byKind.get(name).since === "1.0" ? "" : ` (${byKind.get(name).since})`}`).join(", ")} |`);
  return ["## Contents", "| Group | Kinds |\n|---|---|\n" + rows.join("\n")].join("\n\n");
}

/** A kind: when it was added, its properties, and each of its examples. */
function kind(k, examples) {
  const properties = k.properties.length
    ? k.properties.map((p) => `\`${p.name}\`${p.since === k.since ? "" : ` (${p.since})`}`).join(", ")
    : "none";
  const facts = `API ${k.since}. Properties: ${properties}.`;
  return [`### ${k.kind}`, facts, ...examples.map(example)].join("\n\n");
}

function example(e) {
  const alt = esc(e.about);
  const figure = `<figure class="part"><img class="light" src="gallery/${e.light}" alt="${alt}" loading="lazy"><img class="dark" src="gallery/${e.dark}" alt="${alt}" loading="lazy"></figure>`;
  return [e.about, figure, "```rust\n" + e.code + "\n```"].join("\n\n");
}
