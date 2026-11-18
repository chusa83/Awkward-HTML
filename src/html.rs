//! Minimal, forgiving HTML scanner.
//!
//! It does not build a DOM. It only records where each element lives in the
//! source text, which is what the editor needs to map "element clicked in the
//! design view" back to "range in the code", and to make small edits
//! (attributes, text, insert, delete) without reformatting the rest of the file.

use serde::Serialize;
use std::fmt::Write;

const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];
/// Elements whose content is raw text (no tags inside).
const RAW_TEXT: &[&str] = &["script", "style", "textarea", "title"];
/// Opening one of these closes an open element with the same tag (`<li>a<li>b`).
const CLOSES_SAME: &[&str] = &["li", "p", "option", "tr", "td", "th", "dt", "dd"];
/// Opening one of these closes an open `<p>`.
const CLOSES_P: &[&str] = &[
    "address", "article", "aside", "blockquote", "div", "dl", "fieldset", "footer", "form", "h1",
    "h2", "h3", "h4", "h5", "h6", "header", "hr", "main", "nav", "ol", "p", "pre", "section",
    "table", "ul",
];

/// One element found in the source. All offsets are byte offsets.
#[derive(Debug, Clone)]
pub struct Element {
    pub tag: String,
    pub parent: Option<usize>,
    /// Position of `<` of the start tag.
    pub start: usize,
    /// Position right after the tag name (where we inject `data-dw`).
    pub name_end: usize,
    /// Position right after `>` of the start tag.
    pub open_end: usize,
    /// Position of `<` of the end tag (or `open_end` when there is none).
    pub close_start: usize,
    /// Position right after `>` of the end tag.
    pub end: usize,
    pub has_children: bool,
}

/// What the editor UI receives. Offsets are UTF-16 indexes, which is what
/// JavaScript strings and CodeMirror use.
#[derive(Serialize)]
pub struct ElementInfo {
    pub tag: String,
    pub parent: Option<usize>,
    pub start: usize,
    pub open_end: usize,
    pub close_start: usize,
    pub end: usize,
    /// True when the element only contains text, so it can be edited inline.
    pub text_only: bool,
    pub attrs: Vec<(String, Option<String>)>,
}

pub fn parse(src: &str) -> Vec<Element> {
    let b = src.as_bytes();
    let lower = src.to_ascii_lowercase(); // same byte length as src
    let n = b.len();
    let mut els: Vec<Element> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut i = 0;

    while i < n {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        // Comments, doctype, processing instructions.
        if src[i..].starts_with("<!--") {
            i = find(src, i + 4, "-->").map_or(n, |p| p + 3);
            continue;
        }
        if i + 1 < n && (b[i + 1] == b'!' || b[i + 1] == b'?') {
            i = find(src, i, ">").map_or(n, |p| p + 1);
            continue;
        }
        // End tag.
        if i + 1 < n && b[i + 1] == b'/' {
            let name_end = scan_name(b, i + 2);
            if name_end == i + 2 {
                i += 1;
                continue;
            }
            let name = &lower[i + 2..name_end];
            let gt = find(src, name_end, ">").map_or(n, |p| p + 1);
            if let Some(pos) = stack.iter().rposition(|&e| els[e].tag == name) {
                // Anything still open above the match was closed implicitly.
                for &e in &stack[pos + 1..] {
                    els[e].close_start = i;
                    els[e].end = i;
                }
                let e = stack[pos];
                els[e].close_start = i;
                els[e].end = gt;
                stack.truncate(pos);
            }
            i = gt;
            continue;
        }
        // Start tag.
        if i + 1 < n && b[i + 1].is_ascii_alphabetic() {
            let name_end = scan_name(b, i + 1);
            let tag = lower[i + 1..name_end].to_string();
            let (open_end, self_closing) = scan_tag_end(b, name_end);

            if let Some(&top) = stack.last() {
                let t = els[top].tag.as_str();
                let same = t == tag && CLOSES_SAME.contains(&t);
                let p_closed = t == "p" && CLOSES_P.contains(&tag.as_str());
                if same || p_closed {
                    els[top].close_start = i;
                    els[top].end = i;
                    stack.pop();
                }
            }

            let parent = stack.last().copied();
            if let Some(p) = parent {
                els[p].has_children = true;
            }
            let idx = els.len();
            let mut el = Element {
                tag: tag.clone(),
                parent,
                start: i,
                name_end,
                open_end,
                close_start: open_end,
                end: open_end,
                has_children: false,
            };
            i = open_end;

            if VOID.contains(&tag.as_str()) || self_closing {
                els.push(el);
            } else if RAW_TEXT.contains(&tag.as_str()) {
                let needle = format!("</{tag}");
                let close = find(&lower, open_end, &needle).unwrap_or(n);
                el.close_start = close;
                el.end = find(src, close, ">").map_or(n, |p| p + 1);
                i = el.end;
                els.push(el);
            } else {
                // Unclosed until we see its end tag; EOF closes it.
                el.close_start = n;
                el.end = n;
                els.push(el);
                stack.push(idx);
            }
            continue;
        }
        i += 1;
    }
    els
}

/// Parse the attributes of the start tag of `el`. Values are kept raw
/// (entities are not decoded) so that writing them back is lossless.
pub fn attrs_of(src: &str, el: &Element) -> Vec<(String, Option<String>)> {
    let mut inner = &src[el.name_end..el.open_end.saturating_sub(1).max(el.name_end)];
    inner = inner.trim_end().trim_end_matches('/');
    parse_attrs(inner)
}

pub fn parse_attrs(s: &str) -> Vec<(String, Option<String>)> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
            i += 1;
        }
        let ns = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'=' && b[i] != b'/' {
            i += 1;
        }
        if i == ns {
            i += 1;
            continue;
        }
        let name = s[ns..i].to_string();
        let mut j = i;
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        if j < b.len() && b[j] == b'=' {
            j += 1;
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < b.len() && (b[j] == b'"' || b[j] == b'\'') {
                let q = b[j];
                let vs = j + 1;
                let mut k = vs;
                while k < b.len() && b[k] != q {
                    k += 1;
                }
                out.push((name, Some(s[vs..k].to_string())));
                i = (k + 1).min(b.len());
            } else {
                let vs = j;
                while j < b.len() && !b[j].is_ascii_whitespace() {
                    j += 1;
                }
                out.push((name, Some(s[vs..j].to_string())));
                i = j;
            }
        } else {
            out.push((name, None));
        }
    }
    out
}

/// Element list for the UI, with UTF-16 offsets.
pub fn element_infos(src: &str, els: &[Element]) -> Vec<ElementInfo> {
    let map = utf16_map(src);
    els.iter()
        .map(|e| ElementInfo {
            tag: e.tag.clone(),
            parent: e.parent,
            start: map[e.start],
            open_end: map[e.open_end],
            close_start: map[e.close_start],
            end: map[e.end],
            text_only: is_text_editable(e),
            attrs: attrs_of(src, e),
        })
        .collect()
}

/// Copy of `src` with `data-dw="<index>"` added to every start tag, so the
/// design view can tell which source element a rendered node came from.
pub fn instrument(src: &str, els: &[Element]) -> String {
    let mut out = String::with_capacity(src.len() + els.len() * 16);
    let mut last = 0;
    for (i, e) in els.iter().enumerate() {
        out.push_str(&src[last..e.name_end]);
        let _ = write!(out, " data-dw=\"{i}\"");
        last = e.name_end;
    }
    out.push_str(&src[last..]);
    out
}

// ---------------------------------------------------------------------------
// Source edits. Each returns the new source and the byte offset of the
// element that should be selected afterwards (if any).
// ---------------------------------------------------------------------------

pub fn set_attrs(src: &str, idx: usize, attrs: &[(String, Option<String>)]) -> Result<String, String> {
    let els = parse(src);
    let e = els.get(idx).ok_or("element not found")?;
    let self_closing = src[..e.open_end].ends_with("/>");
    let mut tag = String::from("<");
    tag.push_str(&src[e.start + 1..e.name_end]); // keep original tag-name casing
    for (name, value) in attrs {
        let name = name.trim();
        if name.is_empty() || name.contains(|c: char| c.is_whitespace() || "\"'<>=/".contains(c)) {
            continue;
        }
        match value {
            Some(v) => {
                let _ = write!(tag, " {}=\"{}\"", name, v.replace('"', "&quot;"));
            }
            None => {
                let _ = write!(tag, " {name}");
            }
        }
    }
    tag.push_str(if self_closing { " />" } else { ">" });
    Ok(format!("{}{}{}", &src[..e.start], tag, &src[e.open_end..]))
}

pub fn set_text(src: &str, idx: usize, text: &str) -> Result<String, String> {
    let els = parse(src);
    let e = els.get(idx).ok_or("element not found")?;
    if !is_text_editable(e) {
        return Err("only elements that contain plain text can be edited this way".into());
    }
    let escaped = text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    Ok(format!("{}{}{}", &src[..e.open_end], escaped, &src[e.close_start..]))
}

pub fn delete(src: &str, idx: usize) -> Result<String, String> {
    let els = parse(src);
    let e = els.get(idx).ok_or("element not found")?;
    let (mut s, mut t) = (e.start, e.end);
    // If the element sits alone on its line(s), remove the whole line.
    let line_start = src[..s].rfind('\n').map_or(0, |p| p + 1);
    let line_end = src[t..].find('\n').map_or(src.len(), |p| t + p);
    if src[line_start..s].trim().is_empty() && src[t..line_end].trim().is_empty() {
        s = line_start;
        t = (line_end + 1).min(src.len());
    }
    Ok(format!("{}{}", &src[..s], &src[t..]))
}

/// Insert `html` after element `idx`, or as the last child when `inside`.
/// Returns the new source and the byte offset where the snippet starts.
pub fn insert(src: &str, idx: usize, inside: bool, html: &str) -> Result<(String, usize), String> {
    let els = parse(src);
    let e = els.get(idx).ok_or("element not found")?;
    let indent = line_indent(src, e.start);
    if inside {
        if e.close_start == e.open_end && e.end == e.open_end {
            return Err("this element cannot have children".into());
        }
        let pos = e.close_start;
        let line_start = src[..pos].rfind('\n').map_or(0, |p| p + 1);
        if src[line_start..pos].trim().is_empty() && line_start > e.open_end {
            // End tag on its own line: put the snippet on a new line above it.
            let inner = format!("{indent}  ");
            let text = format!("{inner}{}\n", html.replace('\n', &format!("\n{inner}")));
            let out = format!("{}{}{}", &src[..line_start], text, &src[line_start..]);
            return Ok((out, line_start + indent.len() + 2));
        }
        let out = format!("{}{}{}", &src[..pos], html, &src[pos..]);
        return Ok((out, pos));
    }
    let pos = e.end;
    let text = format!("\n{indent}{}", html.replace('\n', &format!("\n{indent}")));
    let out = format!("{}{}{}", &src[..pos], text, &src[pos..]);
    Ok((out, pos + 1 + indent.len()))
}

/// Index of the element starting at byte offset `pos`, if any.
pub fn index_at(src: &str, pos: usize) -> Option<usize> {
    parse(src).iter().position(|e| e.start == pos)
}

// ---------------------------------------------------------------------------

fn is_text_editable(e: &Element) -> bool {
    !e.has_children
        && e.close_start > e.open_end
        && !VOID.contains(&e.tag.as_str())
        && !RAW_TEXT.contains(&e.tag.as_str())
        && !matches!(e.tag.as_str(), "html" | "head" | "body")
}

fn line_indent(src: &str, pos: usize) -> String {
    let line_start = src[..pos].rfind('\n').map_or(0, |p| p + 1);
    src[line_start..pos].chars().take_while(|c| *c == ' ' || *c == '\t').collect()
}

fn find(hay: &str, from: usize, needle: &str) -> Option<usize> {
    hay.get(from..)?.find(needle).map(|p| p + from)
}

fn scan_name(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'-' || b[i] == b':' || b[i] == b'_') {
        i += 1;
    }
    i
}

/// Returns (position after `>`, ends with `/>`). Respects quoted values.
fn scan_tag_end(b: &[u8], mut i: usize) -> (usize, bool) {
    let mut quote: Option<u8> = None;
    while i < b.len() {
        let c = b[i];
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == b'"' || c == b'\'' => quote = Some(c),
            None if c == b'>' => return (i + 1, i > 0 && b[i - 1] == b'/'),
            None => {}
        }
        i += 1;
    }
    (b.len(), false)
}

/// byte offset -> UTF-16 index, for every byte offset 0..=len.
fn utf16_map(src: &str) -> Vec<usize> {
    let mut map = vec![0; src.len() + 1];
    let mut u = 0;
    for (bi, ch) in src.char_indices() {
        for k in 0..ch.len_utf8() {
            map[bi + k] = u;
        }
        u += ch.len_utf16();
    }
    map[src.len()] = u;
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = "<!doctype html>\n<html>\n<head><title>T <b></title>\n<style>p > a {}</style></head>\n<body>\n  <!-- <div> -->\n  <div id=\"a\" class='x y'>\n    <p>One\n    <p>Two <img src=a.png></p>\n  </div>\n  <ul><li>a<li>b</ul>\n</body>\n</html>\n";

    fn tags(src: &str) -> Vec<String> {
        parse(src).into_iter().map(|e| e.tag).collect()
    }

    #[test]
    fn finds_elements_in_order() {
        assert_eq!(
            tags(PAGE),
            ["html", "head", "title", "style", "body", "div", "p", "p", "img", "ul", "li", "li"]
        );
    }

    #[test]
    fn ranges_and_nesting() {
        let els = parse(PAGE);
        let div = &els[5];
        assert!(PAGE[div.start..div.end].starts_with("<div"));
        assert!(PAGE[div.start..div.end].ends_with("</div>"));
        // first <p> is closed implicitly by the second one
        assert_eq!(&PAGE[els[6].start..els[6].end], "<p>One\n    ");
        assert_eq!(els[7].parent, Some(5));
        assert_eq!(els[8].parent, Some(7));
        assert_eq!(els[11].parent, Some(9));
        assert!(!els[6].has_children);
        assert!(els[7].has_children);
    }

    #[test]
    fn attributes() {
        let els = parse(PAGE);
        let a = attrs_of(PAGE, &els[5]);
        assert_eq!(a, vec![("id".into(), Some("a".into())), ("class".into(), Some("x y".into()))]);
        let a = parse_attrs(" disabled value=3 data-x=\"a>b\" /");
        assert_eq!(a.len(), 3);
        assert_eq!(a[0], ("disabled".into(), None));
        assert_eq!(a[2].1.as_deref(), Some("a>b"));
    }

    #[test]
    fn instrument_marks_every_element() {
        let els = parse(PAGE);
        let out = instrument(PAGE, &els);
        assert_eq!(out.matches("data-dw=").count(), els.len());
        assert!(out.contains("<img data-dw=\"8\" src=a.png>"));
    }

    #[test]
    fn edits() {
        let src = "<body>\n  <h1 class=\"t\">Hi</h1>\n  <p>x</p>\n</body>";
        let out = set_attrs(src, 1, &[("id".into(), Some("main \"q\"".into()))]).unwrap();
        assert!(out.contains("<h1 id=\"main &quot;q&quot;\">Hi</h1>"));
        let out = set_text(src, 1, "a < b").unwrap();
        assert!(out.contains("<h1 class=\"t\">a &lt; b</h1>"));
        let out = delete(src, 2).unwrap();
        assert_eq!(out, "<body>\n  <h1 class=\"t\">Hi</h1>\n</body>");
        let (out, pos) = insert(src, 1, false, "<hr>").unwrap();
        assert!(out.contains("</h1>\n  <hr>\n  <p>"));
        assert_eq!(index_at(&out, pos), Some(2));
        let (out, pos) = insert(src, 0, true, "<div></div>").unwrap();
        assert!(out.ends_with("<p>x</p>\n  <div></div>\n</body>"));
        assert_eq!(parse(&out)[index_at(&out, pos).unwrap()].tag, "div");
        let (out, _) = insert(src, 2, false, "<ul>\n  <li>a</li>\n</ul>").unwrap();
        assert!(out.contains("<p>x</p>\n  <ul>\n    <li>a</li>\n  </ul>\n</body>"));
    }

    #[test]
    fn utf16_offsets() {
        let src = "<p>ñ😀</p><b>x</b>";
        let info = element_infos(src, &parse(src));
        // "<p>" 3 + ñ 1 + 😀 2 + "</p>" 4 = 10
        assert_eq!(info[1].start, 10);
    }
}
