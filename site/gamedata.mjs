// The Game data reference: what the released game map's object model holds, for Lens queries,
// written as one Markdown page from the map's own files (docs/formats/game-map-v2.md#object-model).
import { parse } from "smol-toml";

/** The Markdown of the Game data page, from the files of the released map `version`. */
export function gameData(files, version) {
  const toml = (path) => (files.has(path) ? parse(files.get(path).toString("utf8")) : {});
  const all = (prefix) => [...files.keys()].filter((p) => p.startsWith(prefix) && p.endsWith(".toml")).sort();
  const domains = toml("domains.toml").domains ?? {};
  const roots = toml("roots.toml").roots ?? {};
  const events = toml("events.toml").events ?? {};
  const actions = toml("actions.toml").actions ?? {};
  const types = Object.assign({}, ...all("types/").map((p) => toml(p).types ?? {}));
  const enums = Object.fromEntries(all("enums/").map((p) => [p.slice("enums/".length, -".toml".length), toml(p)]));
  // The value kind of each field entry, for a field whose shape is a plain value.
  const entries = Object.assign({}, ...all("entries/").map((p) => toml(p).field ?? {}));

  const cell = (text) => String(text ?? "").replace(/\|/g, "\\|").replace(/\s+/g, " ").trim();
  const words = (w) => (w ? `${cell(w.en)}` : "");
  const shape = (typeName, name, field) => {
    if (field.text) return "text";
    if (field.ref) return `reference to \`${field.ref}\``;
    if (field.embed) return `\`${field.embed}\``;
    if (field.array) return `list of \`${field.array.of}\``;
    if (field.bit !== undefined) return "bool";
    const kind = entries[field.at ?? `${typeName}.${name}`]?.type ?? "value";
    if (field.sheet) return `row of ${field.sheet}`;
    if (field.enum) return `\`${field.enum}\``;
    if (field.flags) return `flags of \`${field.flags}\``;
    if (field.element) return "window element";
    return field.unit ? `${kind}, ${field.unit}` : kind;
  };
  const change = (field) => {
    if (!field.set) return "";
    const range = field.set.range ? ` ${field.set.range[0]} to ${field.set.range[1]}` : "";
    return `changeable${range}`;
  };

  const out = [];
  out.push(`# Game data`, "");
  out.push(`What game map ${version} describes of the game, for Lens queries: the domains a plugin asks for, the roots a query starts from, the types and their fields, the events a plugin can watch, the actions it can take, and the enums that name values. This page is generated from the released map when the site is built.`, "");

  out.push("## Domains", "", "A plugin lists the domains it reads in `[lens] read`, those it acts in in `[lens] act`, and those it changes in `[lens] change`. The player approves each in these words.", "");
  out.push("| Domain | Class | What the player approves |", "| --- | --- | --- |");
  for (const [name, d] of Object.entries(domains)) {
    const approves = [["Read", d.read], ["Act", d.act], ["Change", d.change]].filter(([, w]) => w).map(([kind, w]) => `**${kind}:** ${words(w)}`).join("<br>");
    out.push(`| \`${name}\` | ${cell(d.class)} | ${approves} |`);
  }
  out.push("");

  out.push("## Roots", "", "A query starts from a root: `player { level }`.", "");
  out.push("| Root | Type | What it is |", "| --- | --- | --- |");
  for (const [name, r] of Object.entries(roots)) out.push(`| \`${name}\` | \`${r.type}\` | ${cell(r.doc)} |`);
  out.push("");

  out.push("## Types", "");
  for (const [name, t] of Object.entries(types).sort(([a], [b]) => a.localeCompare(b))) {
    out.push(`### ${name}`, "", `${cell(t.doc)} Domain \`${t.domain}\`${t.extends ? `; also a \`${t.extends}\`` : ""}.`, "");
    const fields = Object.entries(t.fields ?? {}).filter(([, f]) => !f.forbidden);
    if (fields.length) {
      out.push("| Field | Shape | What it holds |", "| --- | --- | --- |");
      for (const [field, f] of fields) {
        const notes = [f.domain && f.domain !== t.domain ? `domain \`${f.domain}\`` : "", change(f)].filter(Boolean).join("; ");
        out.push(`| \`${field}\` | ${shape(name, field, f)} | ${cell(f.doc)}${notes ? ` (${notes})` : ""} |`);
      }
      out.push("");
    }
  }

  out.push("## Events", "", "A plugin watches an event with a query from it: `action_used { kind, action }`.", "");
  for (const [name, e] of Object.entries(events)) {
    out.push(`### ${name}`, "", `${cell(e.doc)} Domain \`${e.domain}\`.`, "");
    const args = Object.entries(e.args ?? {});
    if (args.length) {
      out.push("| Argument | Shape | What it holds |", "| --- | --- | --- |");
      for (const [arg, a] of args) out.push(`| \`${arg}\` | ${a.enum ? `\`${a.enum}\`` : a.ref ? `reference to \`${a.ref}\`` : a.sheet ? `row of ${a.sheet}` : "value"} | ${cell(a.doc)} |`);
      out.push("");
    }
  }

  out.push("## Actions", "", "A plugin takes an action for the player in a domain it holds with `[lens] act`.", "");
  for (const [name, a] of Object.entries(actions)) {
    out.push(`### ${name}`, "", `${cell(a.doc)} Domain \`${a.domain}\`.`, "");
    // Globals, fixed values, and objects are what Angelica passes itself; the rest are the plugin's.
    const params = Object.entries(a.args ?? {}).filter(([, p]) => p.global === undefined && p.value === undefined && p.object === undefined);
    if (params.length) {
      out.push("| Parameter | Shape | What it is |", "| --- | --- | --- |");
      for (const [param, p] of params) {
        const fallback = p.default === undefined ? "" : ` Left out, it is \`${typeof p.default === "number" && p.default > 255 ? `0x${p.default.toString(16).toUpperCase()}` : p.default}\`.`;
        out.push(`| \`${param}\` | ${p.enum ? `\`${p.enum}\`` : "value"} | ${cell(p.doc)}${fallback} |`);
      }
      out.push("");
    }
  }

  out.push("## Enums", "");
  for (const [name, e] of Object.entries(enums)) {
    out.push(`### ${name}`, "", `${cell(e.doc)}${e.flags ? " A set of flags." : ""}`, "");
    out.push(Object.keys(e.values ?? {}).map((v) => `\`${v}\``).join(", "), "");
  }
  return out.join("\n");
}
