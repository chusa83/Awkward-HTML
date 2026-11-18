//! Local HTTP server.
//!
//! - `/__dw/...`      the editor UI and its JSON API (API needs the session token)
//! - everything else  the site itself, served from disk or from unsaved drafts.
//!   HTML pages are "instrumented" (each element tagged with `data-dw`) so the
//!   design view can map rendered nodes back to source ranges.
//!
//! The site is served at the root so root-relative URLs (`/css/a.css`) work,
//! and the design view iframe is same-origin with the editor, which lets the
//! editor read and highlight the page DOM directly.

use crate::html;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tiny_http::{Header, Method, Request, Response, Server};

pub struct App {
    pub root: PathBuf,
    pub token: String,
    /// Unsaved editor buffers, keyed by site-relative path ("css/site.css").
    drafts: HashMap<String, String>,
}

impl App {
    pub fn new(root: PathBuf, token: String) -> Self {
        App { root, token, drafts: HashMap::new() }
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
        return match api(app, name, &body) {
            Ok(v) => json_resp(&v),
            Err(e) => text(400, &e),
        };
    }
    serve_site(app, &path)
}

fn api(app: &mut App, name: &str, body: &Value) -> Result<Value, String> {
    let path = body["path"].as_str().unwrap_or("").trim_start_matches('/').to_string();
    match name {
        "info" => Ok(json!({
            "root": app.root.display().to_string(),
            "name": app.root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        })),
        "files" => {
            let mut files = Vec::new();
            list_files(&app.root, &app.root, &mut files);
            files.sort();
            Ok(json!({ "files": files, "dirty": app.drafts.keys().collect::<Vec<_>>() }))
        }
        "open" => {
            let content = match app.drafts.get(&path) {
                Some(d) => d.clone(),
                None => {
                    let full = resolve(&app.root, &path).ok_or("bad path")?;
                    let bytes = std::fs::read(&full).map_err(|e| e.to_string())?;
                    String::from_utf8(bytes).map_err(|_| "not a text file")?
                }
            };
            Ok(with_elements(&path, &content, json!({ "content": content })))
        }
        "draft" => {
            let content = body["content"].as_str().ok_or("missing content")?.to_string();
            app.drafts.insert(path.clone(), content.clone());
            Ok(with_elements(&path, &content, json!({})))
        }
        "save" => {
            let content = body["content"].as_str().ok_or("missing content")?;
            let full = resolve(&app.root, &path).ok_or("bad path")?;
            std::fs::write(&full, content).map_err(|e| e.to_string())?;
            app.drafts.remove(&path);
            Ok(json!({}))
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
        _ => Err(format!("unknown api {name}")),
    }
}

fn with_elements(path: &str, content: &str, mut v: Value) -> Value {
    if is_html(path) {
        let els = html::parse(content);
        v["elements"] = json!(html::element_infos(content, &els));
    }
    v
}

fn serve_site(app: &App, url_path: &str) -> Resp {
    let mut rel = url_path.trim_start_matches('/').to_string();
    let Some(mut full) = resolve(&app.root, &rel) else {
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

fn list_files(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        if out.len() > 5000 {
            return;
        }
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        let p = e.path();
        if p.is_dir() {
            list_files(root, &p, out);
        } else if let Ok(rel) = p.strip_prefix(root) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

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

// --- helpers --------------------------------------------------------------

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
