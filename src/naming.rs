//! File-name templating for saved captures.
//!
//! The template is a plain string with two kinds of placeholders:
//!
//! * `$code$` — date and time fields, spelled the way `chrono` formats them
//!   (`yyyy`, `MM`, `dd`, `HH`, `mm`, `ss`, …). A run of characters between two
//!   `$` signs is looked up in [`FIELDS`]; an unknown code is left as written,
//!   so a typo is visible in the preview instead of silently vanishing.
//! * `%name%` — an environment variable (`%USERPROFILE%`) or one of the
//!   special variables in [`SPECIAL`].
//!
//! Anything else is copied verbatim, so a template can hold a folder-like
//! prefix: `shots/$yyyy-MM-dd$/cap-$HH-mm-ss$`.

use std::collections::HashMap;

use chrono::{Datelike, Local, Timelike};

/// Date and time codes usable between `$` signs, longest first.
///
/// The names follow the Windows/Snipaste convention, which is what the naming
/// dialog documents. Descriptions live in the translation table so the panel
/// never shows English in a Chinese UI.
pub const DATE_CODES: &[&str] = &[
    "yyyy", "yy", "MMMM", "MMM", "MM", "M", "dddd", "ddd", "dd", "d", "HH", "H", "hh", "h",
    "mm", "m", "ss", "s", "SSS", "A",
];

/// `%name%` variables that are not in the environment but are still worth
/// naming, plus the environment lookup itself.
pub const VAR_NAMES: &[&str] = &[
    "os", "computername", "username", "userprofile", "title", "title:50",
];

/// The default template: `PinSight_2026-10-24_10-30-00`.
pub const DEFAULT_TEMPLATE: &str = "PinSight_$yyyy-MM-dd_HH-mm-ss$";

/// Expand a template into a file stem.
///
/// `title` is the active window's title, captured by the caller.
pub fn expand(template: &str, title: &str) -> String {
    let mut out = String::with_capacity(template.len() + 32);
    let bytes: Vec<char> = template.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            '$' => {
                // Find the closing `$`. An unterminated `$` is literal text.
                let end = (i + 1..bytes.len()).find(|&j| bytes[j] == '$');
                match end {
                    Some(end) => {
                        let pattern: String = bytes[i + 1..end].iter().collect();
                        // The text between `$` signs is a *pattern*, not a
                        // single code: the default template is
                        // `$yyyy-MM-dd_HH-mm-ss$`. Unknown patterns stay
                        // verbatim so a typo is visible.
                        match expand_pattern(&pattern, title) {
                            Some(expanded) => out.push_str(&expanded),
                            None => {
                                out.push('$');
                                out.push_str(&pattern);
                                out.push('$');
                            }
                        }
                        i = end + 1;
                    }
                    None => {
                        out.push('$');
                        i += 1;
                    }
                }
            }
            '%' => {
                let end = (i + 1..bytes.len()).find(|&j| bytes[j] == '%');
                match end {
                    Some(end) => {
                        let name: String = bytes[i + 1..end].iter().collect();
                        out.push_str(&expand_var(&name, title));
                        i = end + 1;
                    }
                    None => {
                        out.push('%');
                        i += 1;
                    }
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// Expand the text between two `$` signs.
///
/// That text is a *pattern* in the Windows/Snipaste style: the default
/// template wraps `yyyy-MM-dd_HH-mm-ss`, where each token is replaced and the
/// separators are copied through. Returns `None` when the pattern contains no
/// recognised token at all (so a typo stays visible as `$…$`).
fn expand_pattern(pattern: &str, title: &str) -> Option<String> {
    if pattern.is_empty() {
        return Some(String::new());
    }
    // `$title$` / `$title:50$` still work for symmetry with `%title%`.
    if let Some(rest) = pattern.strip_prefix("title") {
        if rest.is_empty() || rest.starts_with(':') {
            let cap = rest
                .strip_prefix(':')
                .and_then(|n| n.parse::<usize>().ok())
                .unwrap_or(usize::MAX);
            return Some(truncate(title, cap));
        }
    }

    // Longest token first, so `MMMM` wins over `MMM`/`MM`/`M` and `HH` over `H`.
    let tokens: Vec<&str> = DATE_CODES.to_vec();

    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::new();
    let mut matched_any = false;
    let mut i = 0;
    'scan: while i < chars.len() {
        for token in &tokens {
            let len = token.chars().count();
            if i + len <= chars.len() && chars[i..i + len].iter().copied().eq(token.chars()) {
                out.push_str(&format_token(token));
                matched_any = true;
                i += len;
                continue 'scan;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    matched_any.then_some(out)
}

/// Format one date/time token, e.g. `yyyy` → `2026`.
///
/// The tokens are the Windows/Snipaste style, so each is mapped to the right
/// `chrono` accessor; the unpadded variants are formatted numerically because
/// `strftime` cannot drop the zero.
fn format_token(token: &str) -> String {
    let now = Local::now();
    match token {
        "yyyy" => format!("{:04}", now.year()),
        "yy" => format!("{:02}", now.year().rem_euclid(100)),
        "MMMM" => now.format("%B").to_string(),
        "MMM" => now.format("%b").to_string(),
        "MM" => format!("{:02}", now.month()),
        "M" => now.month().to_string(),
        "dddd" => now.format("%A").to_string(),
        "ddd" => now.format("%a").to_string(),
        "dd" => format!("{:02}", now.day()),
        "d" => now.day().to_string(),
        "HH" => format!("{:02}", now.hour()),
        "H" => now.hour().to_string(),
        "hh" => format!("{:02}", now.hour12().1),
        "h" => now.hour12().1.to_string(),
        "mm" => format!("{:02}", now.minute()),
        "m" => now.minute().to_string(),
        "ss" => format!("{:02}", now.second()),
        "s" => now.second().to_string(),
        "SSS" => format!("{:03}", now.timestamp_subsec_millis()),
        "A" => now.format("%p").to_string(),
        // Callers only pass tokens from FIELDS; anything else is copied as-is.
        other => other.to_string(),
    }
}

/// Expand one `%name%` variable.
fn expand_var(name: &str, title: &str) -> String {
    if name.is_empty() {
        return String::new();
    }
    if let Some(rest) = name.strip_prefix("title") {
        let cap = rest
            .strip_prefix(':')
            .and_then(|n| n.parse::<usize>().ok())
            .unwrap_or(usize::MAX);
        return truncate(title, cap);
    }
    // `%os%` and friends: an environment variable, or a special name.
    if let Some(value) = special_value(name) {
        return value;
    }
    match std::env::var(name) {
        Ok(v) => v,
        Err(_) => format!("%{name}%"),
    }
}

/// Values that are not environment variables.
fn special_value(name: &str) -> Option<String> {
    let map: HashMap<&str, String> = HashMap::new();
    if let Some(v) = map.get(name) {
        return Some(v.clone());
    }
    match name {
        "os" => Some(std::env::consts::OS.to_string()),
        "computername" => Some(
            std::env::var("COMPUTERNAME")
                .or_else(|_| std::env::var("HOSTNAME"))
                .unwrap_or_default(),
        ),
        "username" => Some(
            std::env::var("USERNAME")
                .or_else(|_| std::env::var("USER"))
                .unwrap_or_default(),
        ),
        _ => None,
    }
}

/// Cut a string to `cap` characters.
fn truncate(text: &str, cap: usize) -> String {
    if text.chars().count() <= cap {
        return text.to_string();
    }
    text.chars().take(cap).collect()
}

/// Strip characters that Windows forbids in a file name.
///
/// The template is expanded first, so a window title may bring in `<`, `:` or
/// a slash; those would make the save fail. `\/:*?"<>|` all become `_`.
pub fn sanitize(stem: &str) -> String {
    let cleaned: String = stem
        .chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches(['.', ' ']).to_string();
    if trimmed.is_empty() {
        "PinSight".to_string()
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_copied() {
        assert_eq!(expand("hello", "t"), "hello");
    }

    #[test]
    fn date_field_expands() {
        let out = expand("$yyyy$", "t");
        assert_eq!(out.len(), 4);
        assert!(out.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn unknown_field_stays_verbatim() {
        assert_eq!(expand("$nope$", "t"), "$nope$");
    }

    #[test]
    fn unterminated_dollar_is_literal() {
        assert_eq!(expand("a$", "t"), "a$");
    }

    #[test]
    fn title_variable_expands() {
        assert_eq!(expand("%title%", "Hello"), "Hello");
        assert_eq!(expand("%title:3%", "Hello"), "Hel");
    }

    #[test]
    fn unknown_variable_stays_verbatim() {
        assert_eq!(expand("%NOPE_XYZ%", "t"), "%NOPE_XYZ%");
    }

    #[test]
    fn sanitize_strips_forbidden_chars() {
        assert_eq!(sanitize("a:b/c*d?e\"f<g>h|i"), "a_b_c_d_e_f_g_h_i");
        assert_eq!(sanitize("trailing.  "), "trailing");
        assert_eq!(sanitize("   "), "PinSight");
    }

    #[test]
    fn default_template_has_a_timestamp() {
        let out = expand(DEFAULT_TEMPLATE, "t");
        assert!(out.starts_with("PinSight_"));
        assert!(out.len() > "PinSight_".len());
    }

    #[test]
    fn default_template_expands_the_whole_pattern() {
        let out = expand(DEFAULT_TEMPLATE, "t");
        // `PinSight_` + `yyyy-MM-dd_HH-mm-ss` (19 chars), with no `$` left.
        assert!(!out.contains('$'), "pattern was not expanded: {out}");
        assert_eq!(out.len(), "PinSight_".len() + 19, "unexpected shape: {out}");
        let stamp = &out["PinSight_".len()..];
        assert!(stamp.chars().next().is_some_and(|c| c.is_ascii_digit()));
    }

    #[test]
    fn composite_pattern_keeps_its_separators() {
        let out = expand("$dd.MM.yyyy$", "t");
        // dd.MM.yyyy -> 8 digits plus two dots.
        assert_eq!(out.len(), 10, "unexpected shape: {out}");
        assert_eq!(out.chars().filter(|c| *c == '.').count(), 2);
    }
}
