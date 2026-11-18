# awkward HTML

<img src="branding/png/icon-128.png" width="96" alt="awkward HTML icon">

A small Dreamweaver-style editor for static websites: code view, design view, property inspector and CSS Designer, kept in sync. Written in Rust, runs on macOS, Windows and Linux.

## Download

Get the latest build from the [Releases](../../releases) page:

- Windows: `AwkwardHTML.exe`
- macOS (Apple Silicon): `AwkwardHTML_macos.zip`
- Linux (x64): `AwkwardHTML_linux_x64.tar.gz`

You need a Chromium-based browser installed (Chrome, Chromium, Edge or Brave); Windows already ships with Edge. The apps are not code signed, so Windows SmartScreen and macOS Gatekeeper will warn on first launch. The release notes explain how to get past that.

## How it works

- `awkward-html` is a single binary. It serves your site folder and the editor UI on `127.0.0.1` and opens the editor in a Chromium-based browser (Chrome, Chromium, Edge or Brave) in app mode, so it looks like a desktop window. The design view is therefore rendered by Chromium on every OS.
- The **source code is always the source of truth**. The design view is your page served with a `data-dw="<n>"` attribute on every element (added on the fly, never written to disk), where `n` is the element's index in the parsed source. That index is how a click in the design view maps back to an exact range in the code.
- Changes made from the design view or the inspector are small text edits to the source (replace a start tag, replace text, insert, delete). The rest of the file is left byte-for-byte as you wrote it: no reformatting.

## Features

### Sites and files
- **Start screen** with recent sites, and an **Open site…** folder browser (with "New folder"). No terminal needed: double-click the program and pick a folder. The program exits when the editor window is closed.
- **File tree** with New file / New folder buttons and a right-click menu: open, rename/move, delete (files and empty folders only), new file/folder here. New `.html` files start from a basic page template.
- Renaming a file does **not** update links that point to it.
- **Save** (Ctrl/Cmd+S) and **Save all** (Ctrl/Cmd+Shift+S). Closing the window with unsaved changes asks for confirmation.

### Design view and inspector

- **Split view**: code and design side by side or stacked; code-only and design-only modes.
- **Click in design → code**: selects the element's start tag in the editor and highlights its whole range.
- **Cursor in code → design**: moving the cursor selects the innermost element under it in the design view and inspector.
- **Tag selector** in the status bar (`<html> <body> <main> <section#intro> <h2>`), click to select an ancestor.
- **Property inspector**: id, class, tag-specific attributes (`href`, `src`, `alt`…), text content, common inline-style properties with the computed value shown as a hint, and a generic attribute list.
- **Edit text in place**: double-click an element that only contains text. Enter commits, Esc cancels.
- **Insert panel**: common elements, inserted after the selection (or inside it).
- **Delete** the selected element with Delete/Backspace in the design view.
- **Live preview of unsaved changes**, including CSS and JS: unsaved files are served from memory, so editing `site.css` updates the open page immediately.
- Root-relative URLs (`/css/site.css`) work because the site is served at `/`.

### CSS Designer
- Lists the rules, from the page's linked stylesheets and `<style>` blocks, whose selectors match the selected element, in source order. "Show all rules" lists every rule.
- Rules inside `@media` are shown with their condition, dimmed when the condition doesn't apply at the current design-view width. Rules that only apply in a state (`:hover`, `::before`…) are labelled as such.
- Edit, add or remove properties; add a new rule (selector suggested from the selection) to any of the page's local stylesheets. Changes are unsaved drafts like any other edit, and the design view updates immediately.
- Only the edited rule's block is rewritten, keeping its one-line or multi-line layout. Comments inside that block are lost; the rest of the file is untouched.

## Run

```bash
cargo run --release -- path/to/your/site
```

Without a folder argument it opens on the start screen.

Options:

- `--port N` to use a fixed port (default: a free one)
- `--no-browser` to only print the editor URL
- `AWKWARD_HTML_BROWSER=/path/to/browser` to override browser detection

Try it with the bundled example:

```bash
cargo run --release -- example
```

`awkward-html --version` prints the version.

### Log file

The program writes a log to `awkward-html.log` in its config folder (the same folder as the recent-sites list):

- Windows: `%APPDATA%\awkward-html\awkward-html.log`
- macOS: `~/Library/Application Support/awkward-html/awkward-html.log`
- Linux: `~/.config/awkward-html/awkward-html.log` (or under `$XDG_CONFIG_HOME`)

It records startup, opened sites, failed operations, crashes, errors in the editor UI and selection mismatches between the design view and the code. Set `AWKWARD_HTML_DEBUG=1` to also log every API call. When the file passes 1 MB it is moved to `awkward-html.log.old`. Attach it to bug reports.

## Versions and releases

Versions are `MAJOR.MINOR.PATCH`: MAJOR marks a completed version, MINOR improvements, PATCH corrections.

To publish a release:

1. Set `version` in `Cargo.toml` (for example `0.1.4`) and push to `main`.
2. On GitHub, open **Actions → Build apps → Run workflow** and enter the same version with a `v` (`v0.1.4`).

The workflow (`.github/workflows/release.yml`) refuses to run if the version doesn't match `Cargo.toml` or the tag already exists. It runs the tests, builds the Windows, macOS and Linux apps and publishes them as a GitHub release. `.github/workflows/ci.yml` runs the tests on every push.

## Icon

The icon sources are in `branding/`: `icon.svg` (48 px and up), `icon-small.svg` (32 px) and `icon-16.svg` (16 px). After changing them, regenerate the PNGs, `icon.ico` and `icon.icns`, and the copies in `ui/`:

```bash
node branding/render-icons.js
```

```bash
python3 branding/make-icon-files.py
```

The first needs Playwright with Chromium, the second Pillow.

## Limitations (current)

- No WYSIWYG typing across elements: in-place editing only works on elements whose content is plain text. Rich content is edited in the code view.
- The CSS Designer doesn't compute specificity or show which declaration wins; rules are listed in source order. Rules with nested CSS, `@font-face` and `@keyframes` are not editable from the panel (use the code view). External (CDN) stylesheets are listed but read-only.
- If a Chromium browser is already running with awkward HTML's profile (a second awkward HTML instance), the program can't tell when its window closes and keeps running until stopped.
- No native menus or native file dialogs: the UI runs in the browser window.
- Files are edited with LF line endings. Files with Windows (CRLF) line endings are converted when opened and saved back with LF.
- One open file at a time (unsaved changes to other files are kept in memory and marked with ●).
- Server-side includes, PHP and Dreamweaver templates (`.dwt`) are not processed; pages are served as they are on disk.
- The HTML scanner is forgiving but not a full HTML5 parser. Very unusual markup can confuse the element mapping; the code view always remains usable.
- No publishing (FTP/SFTP) yet.

## Layout

```
src/main.rs     argument parsing, browser detection and launch
src/server.rs   HTTP server: site files, editor assets, JSON API
src/log.rs      log file (awkward-html.log in the config folder)
src/html.rs     HTML scanner, element ranges, source edits (with tests)
src/css.rs      CSS scanner and rule edits for the CSS Designer (with tests)
ui/             editor UI (plain HTML/CSS/JS, embedded in the binary)
ui/vendor/      CodeMirror 5 (MIT)
branding/       icon sources and generated icon files
build.rs        embeds the icon in the Windows executable
example/        a tiny site to try the editor on
```

## License

awkward HTML is **source-available**, not open source. It's licensed under the [PolyForm Shield License 1.0.0](LICENSE). In plain words (the license text is what counts):

- You may use it for any purpose, including paid work such as building websites for clients.
- You may change it and share copies, including changed copies.
- You may **not** use it, or a changed version of it, to provide a product that competes with awkward HTML. That includes free alternatives and rebranded forks.

Improvements are welcome as pull requests to this repository; see [CONTRIBUTING.md](CONTRIBUTING.md).

### Name and logo

The name "awkward HTML" and the awkward-face logo identify the official project. The software license doesn't grant rights to them. Changed versions must not use the name or logo, and must not suggest they are official.

### Third-party code

CodeMirror 5 (in `ui/vendor/codemirror`) is MIT licensed by its authors; see its `LICENSE` file. The Rust crates the app is built with keep their own licenses.
