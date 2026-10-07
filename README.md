# arcmin

A desktop editor and player for branching interactive stories. arcmin reads and writes
[Arcweave](https://arcweave.com) project exports, so you can bring a story over from Arcweave,
keep editing it here, and play it without any service or size limit.

![icon](assets/icon.png)

## Features

- **Graph editor** – elements, connections and branches on a pan/zoom canvas. Drag from a node's
  green handle to connect, drag the corner to resize, right-click for actions.
- **Variables and scripts** – global variables (integer, float, boolean, string) and Arcscript
  statements in element text. Conditions are syntax-checked as you type.
- **Branches** – `if` / `else if` / `else`. With no labels on its arms a branch routes
  automatically; give arms a *choice label* and the player is offered each labelled arm whose
  condition is true.
- **Covers** – element cover images from the project's assets, shown on nodes and in play mode.
- **Undo / redo**, autosave every 5 minutes, atomic writes.
- **Play mode** with saved progress.

## Install

Download the build for your system from the [releases page](https://github.com/Emincmg/arcmin/releases).

| System  | File | First launch |
|---------|------|--------------|
| macOS   | `arcmin-<version>-macos-universal.dmg` | The app is not notarised: right-click → **Open** the first time. |
| Windows | `arcmin-<version>-windows-x86_64.zip`  | SmartScreen may warn: **More info → Run anyway**. |
| Linux   | `arcmin-<version>-linux-x86_64.tar.gz` | Needs OpenGL/Vulkan and, for file dialogs, `xdg-desktop-portal`. |

Windows and Linux builds are new in 1.0 – please report anything that misbehaves.

### Build from source

Requires Rust 1.88 or newer.

```sh
cargo run --release
```

## Using it

**Getting a story in.** On the start screen choose *Import Arcweave export…* and pick the export's
`project_settings.json`. Its `assets/` folder must sit next to it (unzip an export first). The
project is copied into arcmin's library together with its images, and an import report tells you
how many assets and covers were found. Or type a name and press *Create* for an empty project.

**Where projects live.** Each project is a folder: `project_settings.json`, `assets/`, and two
small files arcmin adds (`*.arcmin-layout.json` for node positions and sizes, `*.arcmin-save.json`
for play progress). *Show in Finder / Explorer* on the start screen opens it.

| System  | Library location |
|---------|------------------|
| macOS   | `~/Library/Application Support/arcmin/projects` |
| Windows | `%APPDATA%\arcmin\projects` |
| Linux   | `$XDG_DATA_HOME/arcmin/projects` (default `~/.local/share/arcmin/projects`) |

Set `ARCMIN_DATA_DIR` to use a different folder.

**Editing.**

| Action | How |
|--------|-----|
| Pan / zoom | Drag the background / scroll |
| Connect | Drag from a node's green handle onto another element or a branch |
| Resize a node | Drag the grip in its lower-right corner |
| Delete selection | `Delete` or `Backspace` |
| Undo / redo | `Cmd`/`Ctrl`+`Z`, `Cmd`/`Ctrl`+`Shift`+`Z` |
| More actions | Right-click the canvas, a node, a branch or a connection |

**Writing text.** Element text and choice labels are plain text with a little markup:

```
A guard blocks the gate. *Very* tired, he looks.     <- *italic*, **bold**

$ if has_key                                          <- lines starting "$ " are Arcscript
You unlock it.
$ gold += 5
$ endif
```

A blank line starts a new paragraph. Variables are referenced by name; manage them in the
**Variables** window. A line starting with `!html ` is kept exactly as written.

**Branches.** Add one with *+ Branch* or by right-clicking a connection → *Insert Branch*. Each
arm has a condition (`has_key`, `hp > 3 and not dead`) and an optional choice label:

- no labels – the player sees one choice and the first true condition is followed automatically;
- some labels – each labelled arm whose condition is true becomes a choice (`else` appears when
  no condition is true). Give a labelled branch an `else` so the player is never left without a choice.

## Arcweave compatibility

arcmin's file format *is* the Arcweave export format. Saving merges your changes into the existing
file, so fields arcmin doesn't edit (notes, jumpers, components, attributes, …) are preserved.

Not editable yet: notes, jumpers, components and attributes, board-scoped variables, switching
between several boards (the main board is edited), importing images into an existing project,
and deleting or renaming projects from the app.

## Development

```sh
cargo test                      # unit tests, including real-runtime playthroughs
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Some tests run against a real export when `ARCMIN_TEST_JSON=/path/to/project_settings.json` is set.
`scripts/package-macos.sh` builds the universal `.app` and `.dmg`. Pushing a `v*` tag builds macOS,
Windows and Linux packages in CI and creates a draft GitHub release with the `gh` CLI.

## License

[MIT](LICENSE)
