# Changelog

## 1.1.0

New editor tools and a Godot export; existing projects open unchanged.

### Editor
- **Auto Arrange** button: lays the board out by title. `2.3` (or `2-3`) is day 2, event 3: days become rows, events become columns. Variants such as `3.3a` / `3.3b` sit to the right of `3.3`, stacked, and never move the day's line. Branches follow the element that leads into them; cards without a `day.event` title go in a block below the grid. One undo step.
- **Smarter connections**: lines are curves with arrowheads, leave and enter the card sides that face each other, and route around other cards instead of running underneath them.
- **Cover images**: right-click a card → *Cover Image* to pick one, and *Upload Image…* (also in the Properties panel) to add a new image. Uploads are copied into the project's `assets/` folder; re-uploading the same file reuses it.
- **Export for Godot…**: writes a `.zip` with `project.json` and, optionally, the `assets/` folder, in the layout Arcweave's own Godot export uses. Not yet tried against a real Godot project.

### Fixes
- Typing a space at the end of a card's title, text, choice label or branch label no longer gets swallowed.

### Under the hood
- New dependency: `zip`.

## 1.0.0

First release.

### Playing and editing
- Opens Arcweave exports (`project_settings.json` with its `assets/` folder) and keeps them in a project library; no file picker is needed afterwards.
- Node-graph editor: add, move, resize and delete elements; draw connections by dragging; edit titles, text and choice labels; pick a starting element; cover images on elements.
- **Variables** window: integer, float, boolean and string variables, with usage counts. Renaming a variable rewrites every script that mentions it.
- **Branches**: `if` / `else if` / `else` with live syntax checking of conditions. A branch either routes automatically (no arm has a label) or offers its labelled arms to the player as choices.
- Script lines in element text (`$ hp += 10`, `$ if hp < 4` … `$ endif`), plus `*italic*` and `**bold**`.
- Undo / redo for everything, right-click menus, per-node resizing, zoom and pan.
- Play mode with autosave and "Continue".

### Safety
- Saving merges your edits into the existing project file instead of rewriting it, so Arcweave data arcmin doesn't model (such as element covers) is never lost; writes are atomic.
- Unsaved changes are written automatically every 5 minutes, when leaving the editor and on exit.

### Platforms
- macOS (universal), Windows and Linux.
