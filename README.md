# Webloom

A small Dreamweaver-style editor for static websites: code view, design view and a property inspector, kept in sync. Written in Rust, runs on macOS, Windows and Linux.

## How it works

- `webloom` is a single binary. It serves your site folder and the editor UI on `127.0.0.1` and opens the editor in a Chromium-based browser (Chrome, Chromium, Edge or Brave) in app mode, so it looks like a desktop window. The design view is therefore rendered by Chromium on every OS.
- The **source code is always the source of truth**. The design view is your page served with a `data-dw="<n>"` attribute on every element (added on the fly, never written to disk), where `n` is the element's index in the parsed source. That index is how a click in the design view maps back to an exact range in the code.
- Changes made from the design view or the inspector are small text edits to the source (replace a start tag, replace text, insert, delete). The rest of the file is left byte-for-byte as you wrote it: no reformatting.

## Features

- **Split view**: code and design side by side or stacked; code-only and design-only modes.
- **Click in design → code**: selects the element's start tag in the editor and highlights its whole range.
- **Cursor in code → design**: moving the cursor selects the innermost element under it in the design view and inspector.
- **Tag selector** in the status bar (`<html> <body> <main> <section#intro> <h2>`), click to select an ancestor.
- **Property inspector**: id, class, tag-specific attributes (`href`, `src`, `alt`…), text content, common inline-style properties with the computed value shown as a hint, and a generic attribute list.
- **Edit text in place**: double-click an element that only contains text. Enter commits, Shift+Enter adds a line break, Esc cancels.
- **Insert panel**: common elements, inserted after the selection (or inside it).
- **Delete** the selected element with Delete/Backspace in the design view.
- **Live preview of unsaved changes**, including CSS and JS: unsaved files are served from memory, so editing `site.css` updates the open page immediately.
- Save with Ctrl/Cmd+S. Root-relative URLs (`/css/site.css`) work because the site is served at `/`.

## Run

```bash
cargo run --release -- path/to/your/site
```

Options:

- `--port N` to use a fixed port (default: a free one)
- `--no-browser` to only print the editor URL
- `WEBLOOM_BROWSER=/path/to/browser` to override browser detection

Try it with the bundled example:

```bash
cargo run --release -- example
```

Prebuilt binaries for the three OSes are produced by GitHub Actions (`.github/workflows/build.yml`) on every push to `main`, and attached to a release when you push a `v*` tag.

## Limitations (current)

- No WYSIWYG typing across elements: in-place editing only works on elements whose content is plain text. Rich content is edited in the code view.
- Only **inline styles** are edited by the inspector. Editing rules in stylesheets (Dreamweaver's CSS Designer) is not implemented yet.
- One open file at a time (unsaved changes to other files are kept in memory and marked with ●).
- Server-side includes, PHP and Dreamweaver templates (`.dwt`) are not processed; pages are served as they are on disk.
- The HTML scanner is forgiving but not a full HTML5 parser. Very unusual markup can confuse the element mapping; the code view always remains usable.
- No publishing (FTP/SFTP) yet.

## Layout

```
src/main.rs     argument parsing, browser detection and launch
src/server.rs   HTTP server: site files, editor assets, JSON API
src/html.rs     HTML scanner, element ranges, source edits (with tests)
ui/             editor UI (plain HTML/CSS/JS, embedded in the binary)
ui/vendor/      CodeMirror 5 (MIT)
example/        a tiny site to try the editor on
```
