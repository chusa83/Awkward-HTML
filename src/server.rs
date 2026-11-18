//! Local HTTP server.
//!
//! - `/__dw/...`      the editor UI and its JSON API (API needs the session token)
//! - everything else  the open site, served from disk or from unsaved drafts.
//!   HTML pages are "instrumented" (each element tagged with `data-dw`) so the
//!   design view can map rendered nodes back to source ranges.
//!
//! The site is served at the root so root-relative URLs (`/css/a.css`) work,
//! and the design view iframe is same-origin with the editor, which lets the
//! editor read and highlight the page DOM directly.

use crate::{css, html, log};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tiny_http::{Header, Method, Request, Response, Server};

pub struct App {
    pub root: Option<PathBuf>,
    pub token: String,
    /// Unsaved editor buffers, keyed by site-relative path ("css/site.css").
    drafts: HashMap<String, String>,
}

impl App {
    pub fn new(root: Option<PathBuf>, token: String) -> Self {
        let mut app = App { root: None, token, drafts: HashMap::new() };
        if let Some(r) = root {
            app.open_site(r);
        }
        app
    }

    fn open_site(&mut self, root: PathBuf) {
        log::info(&format!("open site {}", root.display()));
        remember_recent(&root);
        self.root = Some(root);
        self.drafts.clear();
    }
}

pub fn run(server: Server, mut app: App) {
    for mut req in server.incoming_requests() {
        let res = handle(&mut app, &mut req);
        let _ = req.respond(res);
    }
}

type Resp = Response<std::io::Cursor<Vec<u8>>>;

fn handle(app: &mut App, req: &mut Request) -> Resp {
    let url = req.url().to_string();
    let path = percent_decode(url.split('?').next().unwrap_or("/"));

    if path == "/__dw" || path == "/__dw/" {
        return asset("index.html");
    }
    if let Some(name) = path.strip_prefix("/__dw/static/") {
        return asset(name);
    }
    if let Some(name) = path.strip_prefix("/__dw/api/") {
        let token_ok = req
            .headers()
            .iter()
            .any(|h| h.field.equiv("X-DW-Token") && h.value.as_str() == app.token);
        if !token_ok {
            return text(403, "bad token");
        }
        let mut body = String::new();
        if *req.method() == Method::Post {
            let _ = req.as_reader().read_to_string(&mut body);
        }
        let body: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
        if name != "log" {
            log::debug(&format!("api {name} path={}", body["path"].as_str().unwrap_or("")));
        }
        return match api(app, name, &body) {
            Ok(v) => json_resp(&v),
            Err(e) => {
                log::error(&format!("api {name} path={}: {e}", body["path"].as_str().unwrap_or("")));
                text(400, &e)
            }
        };
    }
    serve_site(app, &path)
}

fn api(app: &mut App, name: &str, body: &Value) -> Result<Value, String> {
    // Calls that work without an open site.
    match name {
        "info" => {
            return Ok(json!({
                "root": app.root.as_ref().map(|r| r.display().to_string()),
                "name": app.root.as_ref().and_then(|r| r.file_name()).map(|n| n.to_string_lossy().to_string()),
                "recent": load_recent(),
                "sep": std::path::MAIN_SEPARATOR.to_string(),
                "version": env!("CARGO_PKG_VERSION"),
            }));
        }
        "log" => {
            log::from_ui(body["level"].as_str().unwrap_or("info"), body["message"].as_str().unwrap_or(""));
            return Ok(json!({}));
        }
        "log_info" => {
            return Ok(json!({ "path": log::path().display().to_string(), "debug": log::debug_enabled() }));
        }
        "browse" => return browse(body["path"].as_str().unwrap_or("")),
        "mkdir_abs" => {
            let parent = PathBuf::from(body["parent"].as_str().ok_or("missing parent")?);
            let name = valid_name(body["name"].as_str().unwrap_or(""))?;
            let p = parent.join(name);
            std::fs::create_dir(&p).map_err(|e| e.to_string())?;
            return Ok(json!({ "path": p.display().to_string() }));
        }
        "open_site" => {
            let p = PathBuf::from(body["path"].as_str().ok_or("missing path")?);
            let p = p.canonicalize().map_err(|e| e.to_string())?;
            if !p.is_dir() {
                return Err("not a folder".into());
            }
            app.open_site(p);
            return Ok(json!({}));
        }
        _ => {}
    }

    let root = app.root.clone().ok_or("no site is open")?;
    let path = clean_rel(body["path"].as_str().unwrap_or(""));
    match name {
        "files" => {
            let mut files = Vec::new();
            let mut dirs = Vec::new();
            list_files(&root, &root, &mut files, &mut dirs);
            files.sort();
            dirs.sort();
            Ok(json!({ "files": files, "dirs": dirs, "dirty": app.drafts.keys().collect::<Vec<_>>() }))
        }
        "open" => {
            let content = read_source(app, &root, &path)?;
            Ok(with_elements(&path, &content, json!({ "content": content })))
        }
        "draft" => {
            let content = body["content"].as_str().ok_or("missing content")?.to_string();
            app.drafts.insert(path.clone(), content.clone());
            Ok(with_elements(&path, &content, json!({})))
        }
        "save" => {
            let content = body["content"].as_str().ok_or("missing content")?;
            let full = resolve(&root, &path).ok_or("bad path")?;
            std::fs::write(&full, content).map_err(|e| e.to_string())?;
            app.drafts.remove(&path);
            Ok(json!({}))
        }
        "save_all" => {
            let mut saved = Vec::new();
            for (p, content) in app.drafts.drain() {
                let full = resolve(&root, &p).ok_or("bad path")?;
                std::fs::write(&full, content).map_err(|e| format!("{p}: {e}"))?;
                saved.push(p);
            }
            Ok(json!({ "saved": saved }))
        }
        "revert" => {
            app.drafts.remove(&path);
            Ok(json!({}))
        }
        "edit" => {
            let content = body["content"].as_str().ok_or("missing content")?;
            let idx = body["index"].as_u64().ok_or("missing index")? as usize;
            let mut select_pos = None;
            let out = match body["op"].as_str().unwrap_or("") {
                "attrs" => {
                    let attrs: Vec<(String, Option<String>)> =
                        serde_json::from_value(body["attrs"].clone()).map_err(|e| e.to_string())?;
                    html::set_attrs(content, idx, &attrs)?
                }
                "text" => html::set_text(content, idx, body["text"].as_str().unwrap_or(""))?,
                "delete" => html::delete(content, idx)?,
                "insert" => {
                    let snippet = body["html"].as_str().ok_or("missing html")?;
                    let inside = body["inside"].as_bool().unwrap_or(false);
                    let (out, pos) = html::insert(content, idx, inside, snippet)?;
                    select_pos = Some(pos);
                    out
                }
                other => return Err(format!("unknown op {other}")),
            };
            let select = select_pos.and_then(|p| html::index_at(&out, p));
            app.drafts.insert(path.clone(), out.clone());
            Ok(with_elements(&path, &out, json!({ "content": out, "select": select })))
        }

        // --- file management ---------------------------------------------
        "create" => {
            let full = resolve(&root, &path).filter(|_| !path.is_empty()).ok_or("bad path")?;
            if full.exists() {
                return Err("a file with that name already exists".into());
            }
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::write(&full, body["content"].as_str().unwrap_or("")).map_err(|e| e.to_string())?;
            Ok(json!({}))
        }
        "mkdir" => {
            let full = resolve(&root, &path).filter(|_| !path.is_empty()).ok_or("bad path")?;
            std::fs::create_dir_all(&full).map_err(|e| e.to_string())?;
            Ok(json!({}))
        }
        "rename" => {
            let to = clean_rel(body["to"].as_str().unwrap_or(""));
            let from_full = resolve(&root, &path).filter(|_| !path.is_empty()).ok_or("bad path")?;
            let to_full = resolve(&root, &to).filter(|_| !to.is_empty()).ok_or("bad target path")?;
            if to_full.exists() {
                return Err("target already exists".into());
            }
            if let Some(parent) = to_full.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::rename(&from_full, &to_full).map_err(|e| e.to_string())?;
            // Move unsaved drafts along with the file or folder.
            let prefix = format!("{path}/");
            let moved: Vec<String> =
                app.drafts.keys().filter(|k| **k == path || k.starts_with(&prefix)).cloned().collect();
            for k in moved {
                let v = app.drafts.remove(&k).unwrap_or_default();
                app.drafts.insert(format!("{to}{}", &k[path.len()..]), v);
            }
            Ok(json!({}))
        }
        "delete" => {
            // Files and empty folders only: deleting a whole folder from a
            // single click is too easy to get wrong.
            let full = resolve(&root, &path).filter(|_| !path.is_empty()).ok_or("bad path")?;
            if full.is_dir() {
                std::fs::remove_dir(&full).map_err(|_| "only empty folders can be deleted here".to_string())?;
            } else {
                std::fs::remove_file(&full).map_err(|e| e.to_string())?;
            }
            app.drafts.remove(&path);
            Ok(json!({}))
        }

        // --- CSS designer -------------------------------------------------
        "css" => {
            let src = read_source(app, &root, &path)?;
            let (css_src, _, _) = css_slice(&src, body)?;
            Ok(json!({ "rules": css::parse(css_src) }))
        }
        "css_edit" => {
            let src = read_source(app, &root, &path)?;
            let (css_src, a, b) = css_slice(&src, body)?;
            let decls: Vec<(String, String)> =
                serde_json::from_value(body["decls"].clone()).map_err(|e| e.to_string())?;
            let (new_css, rule) = if let Some(sel) = body["selector"].as_str() {
                let (indent, tail) = if body["element"].is_u64() {
                    // Inside <style>: indent like the tag, keep </style> on its own line.
                    let style_indent = css::line_indent(&src, src[..a].rfind('<').unwrap_or(0));
                    (format!("{style_indent}  "), style_indent)
                } else {
                    (String::new(), String::new())
                };
                css::add_rule(css_src, sel, &decls, &indent, &tail)?
            } else {
                let idx = body["rule"].as_u64().ok_or("missing rule")? as usize;
                (css::set_decls(css_src, idx, &decls)?, idx)
            };
            let out = format!("{}{}{}", &src[..a], new_css, &src[b..]);
            app.drafts.insert(path.clone(), out.clone());
            Ok(with_elements(&path, &out, json!({ "content": out, "rule": rule })))
        }
        _ => Err(format!("unknown api {name}")),
    }
}

/// The CSS text to work on: the whole file, or the content of a `<style>`
/// element when `body.element` is given. Returns (css, start, end) byte offsets.
fn css_slice<'a>(src: &'a str, body: &Value) -> Result<(&'a str, usize, usize), String> {
    match body["element"].as_u64() {
        None => Ok((src, 0, src.len())),
        Some(i) => {
            let els = html::parse(src);
            let e = els.get(i as usize).ok_or("element not found")?;
            if e.tag != "style" {
                return Err("element is not <style>".into());
            }
            Ok((&src[e.open_end..e.close_start], e.open_end, e.close_start))
        }
    }
}

fn read_source(app: &App, root: &Path, path: &str) -> Result<String, String> {
    if let Some(d) = app.drafts.get(path) {
        return Ok(d.clone());
    }
    let full = resolve(root, path).ok_or("bad path")?;
    let bytes = std::fs::read(&full).map_err(|e| e.to_string())?;
    let text = String::from_utf8(bytes).map_err(|_| "not a text file".to_string())?;
    if text.contains('\r') {
        log::info(&format!("{path}: CR/CRLF line endings converted to LF (saving writes LF)"));
    }
    Ok(normalize_newlines(text))
}

/// Convert CRLF and lone CR line breaks to LF.
///
/// The code editor (CodeMirror) always works with LF internally, while the
/// element offsets sent to the UI are computed from this text. A file saved
/// with Windows line endings would otherwise give offsets that drift one
/// character per line, and selecting in the design view would highlight the
/// wrong code. Saving writes the file back with LF.
fn normalize_newlines(text: String) -> String {
    if !text.contains('\r') {
        return text;
    }
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn with_elements(path: &str, content: &str, mut v: Value) -> Value {
    if is_html(path) {
        let els = html::parse(content);
        v["elements"] = json!(html::element_infos(content, &els));
    }
    v
}

fn serve_site(app: &App, url_path: &str) -> Resp {
    let Some(root) = &app.root else {
        return text(404, "no site is open");
    };
    let mut rel = url_path.trim_start_matches('/').to_string();
    let Some(mut full) = resolve(root, &rel) else {
        return text(400, "bad path");
    };
    if full.is_dir() {
        if !rel.is_empty() && !rel.ends_with('/') {
            rel.push('/');
        }
        rel.push_str("index.html");
        full = full.join("index.html");
    }
    let body: Vec<u8> = match app.drafts.get(&rel) {
        Some(d) => d.clone().into_bytes(),
        None => match std::fs::read(&full) {
            Ok(b) => b,
            Err(_) => return text(404, "not found"),
        },
    };
    let body = if is_html(&rel) {
        let src = String::from_utf8_lossy(&body);
        html::instrument(&src, &html::parse(&src)).into_bytes()
    } else {
        body
    };
    Response::from_data(body)
        .with_header(header("Content-Type", content_type(&rel)))
        .with_header(header("Cache-Control", "no-store"))
}

fn clean_rel(p: &str) -> String {
    p.trim().trim_matches('/').replace('\\', "/")
}

/// Site-relative path -> absolute path, refusing anything that escapes the root.
fn resolve(root: &Path, rel: &str) -> Option<PathBuf> {
    let mut p = root.to_path_buf();
    for comp in rel.split('/') {
        if comp.is_empty() || comp == "." {
            continue;
        }
        if comp == ".." || comp.contains('\\') || comp.contains(':') {
            return None;
        }
        p.push(comp);
    }
    Some(p)
}

fn valid_name(name: &str) -> Result<&str, String> {
    let n = name.trim();
    if n.is_empty() || n == "." || n == ".." || n.contains(['/', '\\', ':']) {
        return Err("invalid name".into());
    }
    Ok(n)
}

fn list_files(root: &Path, dir: &Path, files: &mut Vec<String>, dirs: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        if files.len() > 5000 {
            return;
        }
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        let p = e.path();
        let Ok(rel) = p.strip_prefix(root) else { continue };
        let rel = rel.to_string_lossy().replace('\\', "/");
        if p.is_dir() {
            dirs.push(rel);
            list_files(root, &p, files, dirs);
        } else {
            files.push(rel);
        }
    }
}

// --- folder browser and recent sites ----------------------------------------

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn browse(path: &str) -> Result<Value, String> {
    // On Windows, an empty path past the drive root lists the drives.
    if cfg!(windows) && path == "::drives" {
        let drives: Vec<String> = (b'A'..=b'Z')
            .map(|c| format!("{}:\\", c as char))
            .filter(|d| Path::new(d).exists())
            .collect();
        return Ok(json!({ "path": "", "parent": null, "dirs": drives, "absolute": true }));
    }
    let dir = if path.is_empty() { home_dir() } else { PathBuf::from(path) };
    let dir = dir.canonicalize().map_err(|e| e.to_string())?;
    let mut dirs: Vec<String> = std::fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| !n.starts_with('.'))
        .collect();
    dirs.sort_by_key(|d| d.to_lowercase());
    let parent = match dir.parent() {
        Some(p) => Some(p.display().to_string()),
        None if cfg!(windows) => Some("::drives".to_string()),
        None => None,
    };
    let display = dir.display().to_string();
    // canonicalize() on Windows returns \\?\C:\..., which is noise for people.
    let display = display.strip_prefix(r"\\?\").unwrap_or(&display).to_string();
    Ok(json!({ "path": display, "parent": parent, "dirs": dirs, "absolute": false }))
}

pub fn config_dir() -> PathBuf {
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(home_dir).join("awkward-html")
    } else if cfg!(target_os = "macos") {
        home_dir().join("Library/Application Support/awkward-html")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir().join(".config"))
            .join("awkward-html")
    }
}

fn load_recent() -> Vec<String> {
    std::fs::read_to_string(config_dir().join("recent.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|p| Path::new(p).is_dir())
        .collect()
}

fn remember_recent(root: &Path) {
    let p = root.display().to_string();
    let p = p.strip_prefix(r"\\?\").unwrap_or(&p).to_string();
    let mut list = load_recent();
    list.retain(|x| *x != p);
    list.insert(0, p);
    list.truncate(10);
    let dir = config_dir();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("recent.json"), json!(list).to_string());
}

// --- misc -------------------------------------------------------------------

fn is_html(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    p.ends_with(".html") || p.ends_with(".htm")
}

fn content_type(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "pdf" => "application/pdf",
        "xml" => "application/xml",
        "txt" | "md" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

// --- embedded editor UI ---------------------------------------------------

macro_rules! assets {
    ($($name:literal),* $(,)?) => {
        fn asset_bytes(name: &str) -> Option<&'static [u8]> {
            match name {
                $($name => Some(include_bytes!(concat!("../ui/", $name))),)*
                _ => None,
            }
        }
    };
}

assets!(
    "index.html",
    "app.js",
    "app.css",
    "icon-16.png",
    "icon-32.png",
    "icon-256.png",
    "vendor/codemirror/codemirror.js",
    "vendor/codemirror/codemirror.css",
    "vendor/codemirror/xml.js",
    "vendor/codemirror/javascript.js",
    "vendor/codemirror/css.js",
    "vendor/codemirror/htmlmixed.js",
    "vendor/codemirror/xml-fold.js",
    "vendor/codemirror/matchtags.js",
);

fn asset(name: &str) -> Resp {
    match asset_bytes(name) {
        Some(b) => Response::from_data(b.to_vec())
            .with_header(header("Content-Type", content_type(name)))
            .with_header(header("Cache-Control", "no-store")),
        None => text(404, "not found"),
    }
}

fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).expect("valid header")
}

fn text(status: u16, msg: &str) -> Resp {
    Response::from_data(msg.as_bytes().to_vec())
        .with_status_code(status)
        .with_header(header("Content-Type", "text/plain; charset=utf-8"))
}

fn json_resp(v: &Value) -> Resp {
    Response::from_data(v.to_string().into_bytes())
        .with_header(header("Content-Type", "application/json"))
        .with_header(header("Cache-Control", "no-store"))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("");
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newlines_become_lf() {
        assert_eq!(normalize_newlines("a\r\nb\rc\nd".into()), "a\nb\nc\nd");
        assert_eq!(normalize_newlines("plain\n".into()), "plain\n");
    }

    #[test]
    fn crlf_offsets_match_lf_editor() {
        // What the editor holds after loading a CRLF file is the LF version,
        // so offsets must be computed on that text.
        let crlf = "<html>\r\n<body>\r\n<p>x</p>\r\n</body>\r\n</html>\r\n";
        let lf = normalize_newlines(crlf.into());
        let els = html::parse(&lf);
        let info = html::element_infos(&lf, &els);
        let p = info.iter().find(|e| e.tag == "p").unwrap();
        assert_eq!(&lf[p.start..p.end], "<p>x</p>");
    }
}
