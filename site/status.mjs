// The facts the home page states under its buttons, read from the release feeds when the
// site is built: Angelica's nightly version, the latest map, and the game version its build
// record is for. The feeds do not say which game version is live, so the page never claims
// that Angelica is up to date with the game.
// Usage: import { status } from "./status.mjs"; await status()
import { createHash } from "node:crypto";

const angelica = "https://github.com/AngelicaProject/angelica/releases/download/nightly/channel.json";
const maps = "https://github.com/AngelicaProject/angelica-gamemap/releases";

async function get(url, as) {
  const response = await fetch(url, { redirect: "follow" });
  if (!response.ok) throw new Error(`${url}: ${response.status} ${response.statusText}`);
  return as === "json" ? response.json() : Buffer.from(await response.arrayBuffer());
}

// A feed of update-feed-v1 (docs/formats/update-feed-v1.md) for one product.
function feed(json, product, url) {
  if (json.format !== 1 || json.product !== product || typeof json.version !== "string" || typeof json.published !== "string") {
    throw new Error(`${url}: not a ${product} feed of format 1`);
  }
  return json;
}

// The files of a map pack, by path (docs/formats/update-feed-v1.md#map-pack).
export function unpack(pack) {
  const start = "angelica-pack 1\n";
  if (pack.subarray(0, 16).toString("latin1") !== start) throw new Error("map.pack: not a pack of format 1");
  const files = new Map();
  for (let at = 16; at < pack.length; ) {
    if (at + 4 > pack.length) throw new Error("map.pack: truncated");
    const length = pack.readUInt32LE(at);
    const path = pack.subarray(at + 4, at + 4 + length).toString("utf8");
    at += 4 + length;
    if (at + 8 > pack.length) throw new Error("map.pack: truncated");
    const size = Number(pack.readBigUInt64LE(at));
    at += 8;
    if (at + size > pack.length) throw new Error("map.pack: truncated");
    files.set(path, pack.subarray(at, at + size));
    at += size;
  }
  return files;
}

// The game versions of the build records in a map pack.
export function recordVersions(pack) {
  const versions = [];
  for (const [path, bytes] of unpack(pack)) {
    const record = /^builds\/(.+)\.toml$/.exec(path);
    if (record) {
      const version = /^game_version = "([0-9.]+)"$/m.exec(bytes.toString("utf8"))?.[1];
      if (version !== record[1]) throw new Error(`map.pack: ${path} is not the record of game version ${record[1]}`);
      versions.push(version);
    }
  }
  const order = (v) => v.split(".").map(Number);
  return versions.sort((a, b) => {
    const [x, y] = [order(a), order(b)];
    for (let i = 0; i < Math.max(x.length, y.length); i++) if ((x[i] ?? 0) !== (y[i] ?? 0)) return (x[i] ?? 0) - (y[i] ?? 0);
    return 0;
  });
}

// "2026.10.06.0000.0000" -> "2026-10-06": the date a game version is named by.
const versionDate = (version) => version.split(".").slice(0, 3).join("-");

export async function status() {
  const nightly = feed(await get(angelica, "json"), "angelica", angelica);
  const mapUrl = `${maps}/latest/download/map.json`;
  const map = feed(await get(mapUrl, "json"), "map", mapUrl);
  const file = map.files?.["map.pack"];
  if (!file) throw new Error(`${mapUrl}: the feed names no map.pack`);
  const pack = await get(`${maps}/download/v${map.version}/${file.asset}`);
  if (pack.length !== file.size || createHash("sha256").update(pack).digest("hex") !== file.sha256) {
    throw new Error(`map.pack of ${map.version} is not the file its feed names`);
  }
  const game = recordVersions(pack).at(-1) ?? null;
  return {
    angelica: { version: nightly.version },
    map: { version: map.version, published: map.published },
    game: game && { version: game, date: versionDate(game) },
    // The released map's files, for the Game data reference; not part of the home page's facts.
    files: unpack(pack),
  };
}
