# Design

This folder mirrors the xcreenshot [design canvas](https://claude.ai/artifact/RAcQ8kKKNBJCeC4Z3MriRr). The canvas is where the design is edited; this copy keeps it in version control so changes show up in Git history and anyone can build from it.

Don't edit these files by hand. Change the canvas, then sync it: ask Claude to "sync the design canvas", which reads the latest version, replaces these files and opens a `docs(design):` PR. `sync.json` records the canvas version and a checksum for each file.

**It doesn't update by itself.** After every canvas edit, sync it, or the next UI change is built from the old design. Before any UI work, compare the canvas version with `sync.json` and sync first if they differ.

| Artboard | Screen |
|---|---|
| `Main.dc.html` | Capture overlay, region selected and annotating |
| `WindowSelect.dc.html` | Capture overlay, ready to drag (with magnifier) |
| `Desktop.dc.html` | Mock desktop behind the overlay mockups (not a screen) |
| `Settings.dc.html` | Settings window |
| `Onboarding.dc.html` | First run: Screen Recording permission |
| `Tray.dc.html` | Menu bar menu |
| `UpdateReady.dc.html` | Update ready dialog |
| `ShortcutNotice.dc.html` | Shortcut changed dialog (Windows; both reasons) |
| `Landing.dc.html` | Landing page (GitHub Pages) |
| `AppIcon.dc.html` | App icon options (D, Aperture, is the one shipped) |

The `.dc.html` files are static mockups with inline styles. They need the canvas runtime to render, so open the canvas to see them; read the files for exact values (sizes, colors, copy).
