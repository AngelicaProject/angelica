# Set up an Angelica plugin project

Instructions for a coding agent. The user asked you to set up a project for an Angelica plugin: a plugin for FINAL FANTASY XIV, written in Rust and compiled to a WebAssembly component. Work on Windows, in the terminal you have.

## 1. Check the tools

```shell
cargo --version
git --version
gh --version
gh auth status
where.exe link.exe
```

The work needs Rust (`cargo`), Git, the GitHub command `gh` signed in, and the Microsoft C++ build tools, which Rust uses on Windows to build the test host.

## 2. Offer to install what is missing

Tell the user in one message what is missing and that you can install it, and wait for their answer. With their agreement, install with `winget`:

| Missing | Command |
|---|---|
| Rust | `winget install --id Rustlang.Rustup -e` |
| Git | `winget install --id Git.Git -e` |
| `gh` | `winget install --id GitHub.cli -e` |
| C++ build tools | `winget install --id Microsoft.VisualStudio.2022.BuildTools -e --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"` |

Open a new terminal after installing, so the new commands are found. If the user prefers to install themselves, give them the same list and wait.

`gh auth login` needs the user: start it, and ask them to finish signing in in the browser it opens.

## 3. Install the `angelica` command

```shell
cargo install --locked --git https://github.com/AngelicaProject/angelica angelica-cli
```

## 4. Agree on the plugin

If the user has not said what the plugin should do, ask now. Then agree on the plugin's ID with them. It is permanent and is also the plugin's chat command: lowercase letters, digits, and inner hyphens, starting with a letter, at most 32 characters.

## 5. Create the project

In the folder the user chooses:

```shell
angelica plugin new <id>
cd <id>
cargo test
```

The first `cargo test` fetches the Angelica repository and builds the test host, which takes a few minutes. All tests pass on a new project.

## 6. Continue with the project's AGENTS.md

Read `AGENTS.md` in the project and follow its work loop from the first step: the design you agree on with the user. It holds the commands, the rules, and links to the documentation, and it stays with the project for every later change.
