# Website

The website at `https://angelicaproject.github.io/angelica/` is the home page for players and the documentation written for people. It is built from `site/` and published to GitHub Pages by `.github/workflows/pages.yml` after every push to `main` that changes `site/`, and every hour.

## What it holds

- `site/home.mjs` builds the home page; `site/plates.mjs` holds the drawings of its engine section.
- `site/status.mjs` reads the release feeds for the home page's status: Angelica's nightly version (`channel.json` of the nightly release), the latest map's version and date (`map.json` of the latest `angelica-gamemap` release), and the newest game version a build record in that map's `map.pack` is for, after checking the pack's size and SHA-256 against its feed. The build reads them because release assets cannot be fetched by a browser, and fails when a feed cannot be read. The status says what the feeds gave; the feeds do not say which game version is live, so it never claims that Angelica is up to date with the game.
- `site/pages/` holds the documentation pages, one Markdown file each. `site/docs.mjs` turns a page into HTML.
- `site/art/` holds the lily: `lily-photo.png` is the generated photograph it comes from, and `tones.png`, `detail.png`, and `ground.png` are the masks the home page prints it with. `site/tools/` makes the masks from the photograph.
- `site/build.mjs` builds everything into `site/dist/` and needs the network for the feeds: `index.html`, an HTML and a Markdown copy of every page, and `llms.txt`, which lists every page as Markdown and the canonical documents a plugin author needs.

```text
cd site
npm ci
node build.mjs
```

Every **Download** link on the site points to `releases/download/nightly/angelica-setup.exe`, the installer of the nightly channel ([releases](./releases.md)).

The build fails when a link inside the site leads to a missing file or a missing element of a page. The `site` job of [CI](./ci.md) runs it for every pull request.

## Pages for people and canonical documents

`docs/` stays the source of truth for behavior, formats, and rules ([documentation](./documentation.md)). A page in `site/pages/` explains a task or an idea to a reader and links to the canonical document instead of restating a contract. When behavior that a page describes changes, the page changes in the same pull request as the canonical document.

The menu of the documentation follows the home page's three shelves: **Play** for players, **Build** for plugin authors, **Understand** for how Angelica works and how to contribute. A shelf lists only pages that exist; until a page for people exists, its entry links to the canonical document on GitHub.

## Writing a page

A page starts with its facts between `---` lines, then a `#` title and one paragraph that says what the page is for:

```markdown
---
order: 2
section: Play
kicker: Get started
meta: About 1 minute · In the game
prev: Install Angelica
next: Patch day
---

# Turn on a plugin
```

`section` is `Play` or `Build`; `order` places the page in its section and in `llms.txt`; `kicker` is the word above the title; `meta` is a few facts under it, separated by ` · `; `prev` and `next` are titles of other pages. A page links to another page by its Markdown file (`[Patch day](patch-day.md)`).

Pages follow the topic types of the GitLab documentation: a tutorial teaches by building something, a task is numbered steps toward one goal, a concept explains an idea, and troubleshooting starts from what the reader sees. Player pages use the words of the interface, in bold, exactly as the menu shows them, and leave out how Angelica works inside. Every sentence says something the reader needs; a sentence that only reassures is removed.

## Rebuilding the lily

The masks are made from the photograph with Python, Pillow, and NumPy:

```text
cd site
python -I tools/remake.py art/lily-photo.png art
python tools/treat.py art/thin.png art 700 0.02,0.06,0.98,0.72
python -I tools/order.py art
```

`remake.py` narrows the stems, crops the frame, and writes `detail.png`, `outline.png`, and `solid.png`; `treat.py` prints the photograph as `tones.png`; `order.py` writes `ground.png`. Only `tones.png`, `detail.png`, and `ground.png` are kept.
