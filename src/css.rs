//! Minimal CSS scanner for the CSS Designer.
//!
//! Finds style rules (including the ones inside @media / @supports / @container
//! / @layer blocks) and the byte range of each declaration block, so a single
//! rule can be rewritten without touching the rest of the stylesheet.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Rule {
    pub selector: String,
    /// Enclosing conditional at-rules, e.g. "@media (max-width: 600px)".
    pub media: Option<String>,
    pub decls: Vec<(String, String)>,
    /// The block contains nested rules; editing it would lose them.
    pub nested: bool,
    #[serde(skip)]
    pub start: usize,
    /// Position right after `{`.
    #[serde(skip)]
    pub block_start: usize,
    /// Position of the closing `}` (or end of input).
    #[serde(skip)]
    pub block_end: usize,
}

pub fn parse(src: &str) -> Vec<Rule> {
    let mut out = Vec::new();
    parse_block(src, 0, src.len(), None, &mut out);
    out
}

fn parse_block(src: &str, mut i: usize, end: usize, media: Option<&str>, out: &mut Vec<Rule>) {
    let b = src.as_bytes();
    while i < end {
        let c = b[i];
        if c.is_ascii_whitespace() || c == b'}' {
            i += 1;
            continue;
        }
        if src[i..end].starts_with("/*") {
            i = find(src, i + 2, "*/").map_or(end, |p| p + 2);
            continue;
        }
        if src[i..end].starts_with("<!--") || src[i..end].starts_with("-->") {
            i += if b[i] == b'<' { 4 } else { 3 };
            continue;
        }
        let start = i;
        let stop = scan_until(b, i, end, |c| c == b'{' || c == b';');
        if stop >= end || b[stop] == b';' {
            i = stop + 1; // @import, @charset, stray text
            continue;
        }
        let prelude = collapse_ws(&strip_comments(&src[start..stop]));
        let block_start = stop + 1;
        let block_end = matching_brace(b, stop, end);
        if let Some(rest) = prelude.strip_prefix('@') {
            let name: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
            if matches!(name.to_ascii_lowercase().as_str(), "media" | "supports" | "container" | "layer" | "document" | "scope") {
                let label = match media {
                    Some(m) => format!("{m} {prelude}"),
                    None => prelude.clone(),
                };
                parse_block(src, block_start, block_end, Some(&label), out);
            }
            // @font-face, @keyframes, @page...: not shown.
        } else {
            let body = &src[block_start..block_end];
            let clean = strip_comments(body);
            out.push(Rule {
                selector: prelude,
                media: media.map(str::to_string),
                nested: clean.contains('{'),
                decls: parse_decls(&clean),
                start,
                block_start,
                block_end,
            });
        }
        i = block_end + 1;
    }
}

/// `color: red; background: url("a;b.png")` -> [(color, red), (background, url(...))]
pub fn parse_decls(s: &str) -> Vec<(String, String)> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let stop = scan_until(b, i, b.len(), |c| c == b';');
        let part = &s[i..stop.min(s.len())];
        if let Some(colon) = part.find(':') {
            let prop = part[..colon].trim();
            let value = part[colon + 1..].trim();
            if !prop.is_empty() && !prop.contains('{') && !value.contains('{') {
                out.push((prop.to_string(), value.to_string()));
            }
        }
        i = stop + 1;
    }
    out
}

/// Replace the declarations of rule `idx`, keeping the block's layout style
/// (one declaration per line, or all on one line).
pub fn set_decls(src: &str, idx: usize, decls: &[(String, String)]) -> Result<String, String> {
    let rules = parse(src);
    let r = rules.get(idx).ok_or("rule not found")?;
    if r.nested {
        return Err("this rule contains nested rules; edit it in the code view".into());
    }
    let decls = clean_decls(decls)?;
    let old = &src[r.block_start..r.block_end];
    let block = if old.contains('\n') || old.trim().is_empty() {
        let rule_indent = line_indent(src, r.start);
        let indent = old
            .lines()
            .skip(1)
            .find(|l| !l.trim().is_empty())
            .map(|l| l[..l.len() - l.trim_start().len()].to_string())
            .filter(|s| s.len() > rule_indent.len())
            .unwrap_or_else(|| format!("{rule_indent}  "));
        let mut s = String::from("\n");
        for (p, v) in &decls {
            s.push_str(&format!("{indent}{p}: {v};\n"));
        }
        s.push_str(&rule_indent);
        s
    } else if decls.is_empty() {
        " ".to_string()
    } else {
        let parts: Vec<String> = decls.iter().map(|(p, v)| format!("{p}: {v};")).collect();
        format!(" {} ", parts.join(" "))
    };
    Ok(format!("{}{}{}", &src[..r.block_start], block, &src[r.block_end..]))
}

/// Append a new rule at the end of the stylesheet. Returns the new source and
/// the index of the new rule.
pub fn add_rule(src: &str, selector: &str, decls: &[(String, String)], indent: &str, tail: &str) -> Result<(String, usize), String> {
    let selector = collapse_ws(selector);
    if selector.is_empty() || selector.contains(['{', '}', ';']) {
        return Err("invalid selector".into());
    }
    let decls = clean_decls(decls)?;
    let mut rule = format!("{indent}{selector} {{\n");
    for (p, v) in &decls {
        rule.push_str(&format!("{indent}  {p}: {v};\n"));
    }
    rule.push_str(&format!("{indent}}}\n"));
    let head = src.trim_end();
    let sep = if head.is_empty() { "\n" } else { "\n\n" };
    let out = format!("{head}{sep}{rule}{tail}");
    let idx = parse(&out).len().checked_sub(1).ok_or("could not add rule")?;
    Ok((out, idx))
}

fn clean_decls(decls: &[(String, String)]) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    for (p, v) in decls {
        let p = p.trim();
        let v = v.trim().trim_end_matches(';').trim();
        if p.is_empty() || v.is_empty() {
            continue;
        }
        if p.contains(|c: char| c.is_whitespace() || ":;{}".contains(c)) {
            return Err(format!("invalid property name: {p}"));
        }
        if v.contains(['{', '}']) {
            return Err(format!("invalid value for {p}"));
        }
        out.push((p.to_string(), v.to_string()));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------

/// First index >= i where `stop(byte)` is true at top level (outside strings,
/// comments and parentheses), or `end`.
fn scan_until(b: &[u8], mut i: usize, end: usize, stop: impl Fn(u8) -> bool) -> usize {
    let mut depth = 0;
    while i < end {
        let c = b[i];
        if c == b'"' || c == b'\'' {
            i = skip_string(b, i, end);
            continue;
        }
        if c == b'/' && i + 1 < end && b[i + 1] == b'*' {
            i = skip_comment(b, i, end);
            continue;
        }
        if c == b'(' {
            depth += 1;
        } else if c == b')' && depth > 0 {
            depth -= 1;
        } else if depth == 0 && stop(c) {
            return i;
        }
        i += 1;
    }
    end
}

/// `open` is the index of `{`; returns the index of the matching `}` or `end`.
fn matching_brace(b: &[u8], open: usize, end: usize) -> usize {
    let mut depth = 0;
    let mut i = open;
    while i < end {
        let c = b[i];
        if c == b'"' || c == b'\'' {
            i = skip_string(b, i, end);
            continue;
        }
        if c == b'/' && i + 1 < end && b[i + 1] == b'*' {
            i = skip_comment(b, i, end);
            continue;
        }
        if c == b'{' {
            depth += 1;
        } else if c == b'}' {
            depth -= 1;
            if depth == 0 {
                return i;
            }
        }
        i += 1;
    }
    end
}

fn skip_string(b: &[u8], i: usize, end: usize) -> usize {
    let q = b[i];
    let mut j = i + 1;
    while j < end && b[j] != q && b[j] != b'\n' {
        if b[j] == b'\\' {
            j += 1;
        }
        j += 1;
    }
    (j + 1).min(end)
}

fn skip_comment(b: &[u8], i: usize, end: usize) -> usize {
    let mut j = i + 2;
    while j + 1 < end && !(b[j] == b'*' && b[j + 1] == b'/') {
        j += 1;
    }
    (j + 2).min(end)
}

fn strip_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find("/*") {
        out.push_str(&rest[..p]);
        out.push(' ');
        rest = rest[p + 2..].find("*/").map_or("", |q| &rest[p + 2 + q + 2..]);
    }
    out.push_str(rest);
    out
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn find(hay: &str, from: usize, needle: &str) -> Option<usize> {
    hay.get(from..)?.find(needle).map(|p| p + from)
}

pub fn line_indent(src: &str, pos: usize) -> String {
    let line_start = src[..pos].rfind('\n').map_or(0, |p| p + 1);
    src[line_start..pos].chars().take_while(|c| *c == ' ' || *c == '\t').collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CSS: &str = "@charset \"utf-8\";\n/* header */\nbody { margin: 0; color: #222 }\n.top a,\n.top b {\n  color: red;\n  background: url(\"x;y.png\");\n}\n@media (max-width: 600px) {\n  .top { display: none; }\n}\n@font-face { font-family: X; src: url(x.woff); }\n.a { .b { color: red } }\n";

    #[test]
    fn finds_rules() {
        let r = parse(CSS);
        let sel: Vec<_> = r.iter().map(|r| r.selector.as_str()).collect();
        assert_eq!(sel, ["body", ".top a, .top b", ".top", ".a"]);
        assert_eq!(r[1].decls[1], ("background".into(), "url(\"x;y.png\")".into()));
        assert_eq!(r[2].media.as_deref(), Some("@media (max-width: 600px)"));
        assert!(r[3].nested);
        assert!(!r[0].nested);
    }

    #[test]
    fn edits_keep_layout() {
        let d = |p: &str, v: &str| (p.to_string(), v.to_string());
        let out = set_decls(CSS, 0, &[d("margin", "0"), d("color", "blue;")]).unwrap();
        assert!(out.contains("body { margin: 0; color: blue; }\n"));
        let out = set_decls(CSS, 1, &[d("color", "green")]).unwrap();
        assert!(out.contains(".top b {\n  color: green;\n}\n@media"));
        let out = set_decls(CSS, 2, &[d("display", "block"), d("gap", "1rem")]).unwrap();
        assert!(out.contains("  .top { display: block; gap: 1rem; }\n}"));
        assert!(set_decls(CSS, 3, &[]).is_err());
        assert!(set_decls(CSS, 0, &[d("color", "red} body {")]).is_err());
        // Everything else untouched.
        let out = set_decls(CSS, 1, &[d("color", "red"), d("background", "url(\"x;y.png\")")]).unwrap();
        assert_eq!(out, CSS);
    }

    #[test]
    fn adds_rules() {
        let (out, idx) = add_rule("p { color: red }\n", "#main", &[("gap".into(), "4px".into())], "", "").unwrap();
        assert_eq!(out, "p { color: red }\n\n#main {\n  gap: 4px;\n}\n");
        assert_eq!(idx, 1);
        assert_eq!(parse(&out)[idx].selector, "#main");
        let (out, _) = add_rule("", ".x", &[], "  ", "  ").unwrap();
        assert_eq!(out, "\n  .x {\n  }\n  ");
    }
}
