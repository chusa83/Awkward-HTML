# Notes for Claude

## "PR and merge" means release too

When asked to open a PR and merge, always finish with a release. This applies to every change, including docs-only or CI-only ones. Steps:

1. Bump `version` in `Cargo.toml` on the branch before the PR, following the rule in `.github/workflows/release.yml`:
   MAJOR = completed version, MINOR = improvements, PATCH = corrections.
   If unsure whether a change is an improvement or a correction, ask. Run `cargo build` so `Cargo.lock` picks up the new version, and update the `default:` tag in `release.yml` to match.
2. Run `cargo test`, commit, push, open the PR (fill in `.github/pull_request_template.md`).
3. Wait for the `Tests` check to pass, then merge.
4. Run the "Build apps" workflow (`release.yml`) on `main` with the version tag, e.g. `v0.1.5`:
   `gh api -X POST repos/chusa83/Awkward-HTML/actions/workflows/release.yml/dispatches -f ref=main -f 'inputs[version]=v0.1.5'`
5. Wait for it to finish and confirm the release has `AwkwardHTML.exe`, `AwkwardHTML_macos.zip`, `AwkwardHTML_linux_x64.tar.gz` and `AwkwardHTML_LICENSE.txt`. Report the release link.

In Claude Code cloud sessions `gh pr create` fails (GraphQL is blocked); use the REST API (`gh api repos/chusa83/Awkward-HTML/pulls ...`, `.../pulls/N/merge`).

## Project

- Run tests with `cargo test`. Try UI changes with `cargo run -- example`.
- The UI files in `ui/` are embedded in the binary at build time: rebuild after editing them.
- The code editor (CodeMirror) works with LF. Files are converted to LF when read (`read_source` in `src/server.rs`), and element offsets sent to the UI are UTF-16 indexes into that LF text.
- Log file: `awkward-html.log` in the config folder (see `src/log.rs`). `AWKWARD_HTML_DEBUG=1` logs every API call.
