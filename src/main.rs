//! Webloom: a small Dreamweaver-style site editor.
//!
//! The Rust binary serves the editor UI and the site on 127.0.0.1 and opens
//! it in a Chromium-based browser (Chrome, Chromium, Edge, Brave) in "app"
//! mode, so it looks like a desktop window.
//!
//! Usage: webloom [SITE_FOLDER] [--port N] [--no-browser]

mod html;
mod server;

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let mut site: Option<PathBuf> = None;
    let mut port: u16 = 0;
    let mut open_browser = true;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--port" => port = args.next().and_then(|p| p.parse().ok()).unwrap_or(0),
            "--no-browser" => open_browser = false,
            "-h" | "--help" => {
                println!("Usage: webloom [SITE_FOLDER] [--port N] [--no-browser]");
                println!("Set WEBLOOM_BROWSER to the path of a Chromium-based browser to override detection.");
                return;
            }
            _ => site = Some(PathBuf::from(a)),
        }
    }
    let site = site.unwrap_or_else(|| std::env::current_dir().expect("current dir"));
    let root = match site.canonicalize() {
        Ok(p) if p.is_dir() => p,
        _ => {
            eprintln!("Not a folder: {}", site.display());
            std::process::exit(1);
        }
    };

    let server = tiny_http::Server::http(("127.0.0.1", port)).unwrap_or_else(|e| {
        eprintln!("Could not start server: {e}");
        std::process::exit(1);
    });
    let port = server.server_addr().to_ip().map(|a| a.port()).unwrap_or(port);
    let token = random_token();
    let url = format!("http://127.0.0.1:{port}/__dw/#{token}");

    println!("Site:   {}", root.display());
    println!("Editor: {url}");

    if open_browser {
        if let Err(e) = launch_browser(&url) {
            eprintln!("{e}\nOpen the editor URL above in Chrome, Chromium or Edge.");
        }
    }
    server::run(server, server::App::new(root, token));
}

/// Token that the editor must send with API calls, so other local pages
/// can't read or write files through the server.
fn random_token() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut s = String::new();
    for i in 0..2 {
        let mut h = RandomState::new().build_hasher();
        h.write_u64(i);
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        );
        h.write_u32(std::process::id());
        s.push_str(&format!("{:016x}", h.finish()));
    }
    s
}

fn launch_browser(url: &str) -> Result<(), String> {
    let exe = find_browser().ok_or("No Chromium-based browser found (set WEBLOOM_BROWSER).")?;
    // Separate profile so the window opens even if the browser is already running,
    // and so the editor never touches the user's normal browsing profile.
    let profile = std::env::temp_dir().join("webloom-profile");
    Command::new(&exe)
        .arg(format!("--app={url}"))
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--window-size=1400,900")
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not start {}: {e}", exe.display()))
}

fn find_browser() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("WEBLOOM_BROWSER") {
        return Some(PathBuf::from(p));
    }
    let candidates: &[&str] = if cfg!(target_os = "macos") {
        &[
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
        ]
    } else if cfg!(windows) {
        &[
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
            r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        ]
    } else {
        &[
            "google-chrome",
            "google-chrome-stable",
            "chromium",
            "chromium-browser",
            "microsoft-edge",
            "brave-browser",
        ]
    };
    for c in candidates {
        let p = Path::new(c);
        if p.is_absolute() {
            if p.exists() {
                return Some(p.to_path_buf());
            }
        } else if let Some(found) = which(c) {
            return Some(found);
        }
    }
    if cfg!(windows) {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let p = PathBuf::from(local).join(r"Google\Chrome\Application\chrome.exe");
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(name)).find(|p| p.is_file())
}
