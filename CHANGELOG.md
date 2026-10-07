# Changelog

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
