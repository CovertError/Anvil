//! Escape primitives. Askama already escapes `{{ }}` by default; `{!! !!}` opts out via `|safe`.

/// HTML-escape a string. (Convenience — Askama uses its own escaper by default.)
pub fn html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            other => out.push(other),
        }
    }
    out
}

/// Mark a string as safe — Askama receives this through `|safe` to opt out of escaping.
pub fn safe(input: impl Into<String>) -> SafeString {
    SafeString(input.into())
}

#[derive(Debug, Clone)]
pub struct SafeString(pub String);

impl std::fmt::Display for SafeString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
