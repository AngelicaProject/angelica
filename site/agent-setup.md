# Set up an Angelica plugin project

Instructions for a coding agent. The user asked you to set up a project for an Angelica plugin: a plugin for FINAL FANTASY XIV (FFXIV), written in Rust and compiled to a WebAssembly component. Work on Windows, in the terminal you have.

## 1. Check the tools

The work needs:

- Rust: `cargo --version`.
- Git: `git --version`.
- The GitHub command, signed in: `gh auth status`. The work uses it to look up and file requests for capabilities Angelica lacks, and to publish the plugin.
- The Microsoft C++ build tools, which Rust uses on Windows to build the test host. Rust finds them without `PATH`; check with:

  ```shell
  & "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe" -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
  ```

  It prints a folder when they are installed. A `link.exe` on `PATH` proves nothing: Git ships an unrelated program with that name.

## 2. Offer to install what is missing

Tell the user in one message what is missing and that you can install it, and wait for their answer. With their agreement, install with `winget`:

| Missing | Command |
|---|---|
| Rust | `winget install --id Rustlang.Rustup -e` |
| Git | `winget install --id Git.Git -e` |
| `gh` | `winget install --id GitHub.cli -e` |
| C++ build tools | `winget install --id Microsoft.VisualStudio.2022.BuildTools -e --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"` |

Open a new terminal after installing, so the new commands are found. If the user prefers to install themselves, give them the same list and wait.

`gh auth login` needs the user: start it, and ask them to finish signing in in the browser it opens. If `gh` fails for another reason, show the user its message instead of reinstalling it.

## 3. Install the `angelica` command

Run this even when `angelica` is already installed, so the command matches the documentation:

```shell
cargo install --locked --git https://github.com/AngelicaProject/angelica angelica-cli
```

It builds the command from source, which takes a few minutes; when the installed command is already the latest, cargo says so and builds nothing. Cargo puts it into `%USERPROFILE%\.cargo\bin`, which the Rust installer adds to `PATH`; open a new terminal if `angelica` is not found.

## 4. Agree on the plugin

If the user has not said what the plugin should do, ask now. Then agree on the plugin's ID with them. It is permanent, and it is the plugin's chat command: a plugin with the ID `timers` answers `/timers`, and its other commands are words after it, such as `/timers add`. The ID uses lowercase letters, digits, and inner hyphens, starts with a letter, and has at most 32 characters.

## 5. Create the project

`angelica plugin new <id>` creates the folder `<id>` inside the current folder. In the folder the user chooses:

```shell
angelica plugin new <id>
cd <id>
git init
cargo test
```

The first `cargo test` fetches the Angelica repository and builds the test host, which takes a few minutes. All tests pass on a new project; if they do not, show the user the first error. The project becomes the plugin's own GitHub repository when it is published.

## 6. Continue with the project's AGENTS.md

Read `AGENTS.md` in the project and follow its work loop from the first step: the design you agree on with the user. It holds the commands, the rules, and links to the documentation, and it stays with the project for every later change.

## Later plugins

Angelica's agent skill, `ffxiv-plugin`, points an agent to these instructions whenever the user asks for an FFXIV plugin. Mention it once the first plugin works, and install it only if the user wants it:

- Claude Code: `claude plugin marketplace add AngelicaProject/angelica`, then `claude plugin install angelica@angelica`.
- Other agents that read Agent Skills: `gh skill install AngelicaProject/angelica ffxiv-plugin`.
