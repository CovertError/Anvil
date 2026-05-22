//! Parser for Forge `.forge.html` files. Produces a flat token stream that the
//! lowering pass converts to Askama syntax.
//!
//! We use a simple linear scanner rather than `chumsky` here because Forge's
//! grammar is mostly token-by-token rewrites — a recursive parser would be
//! overkill and harder to debug for the POC.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// Literal HTML/text content.
    Text(String),

    /// `{{ expr }}` — escaped output.
    EscapedExpr(String),

    /// `{!! expr !!}` — unescaped output.
    RawExpr(String),

    /// `@{{ expr }}` — Blade-style escape: output the literal text
    /// `{{ expr }}` without interpolation. Used to embed mustache syntax
    /// in client-side templates (Vue, Alpine, Handlebars) that share Blade's
    /// brace conventions.
    LiteralExpr(String),

    /// `@@name` or `@@name(args)` — Blade-style directive escape: output the
    /// literal `@name(args)` text without executing the directive.
    LiteralDirective { name: String, args: Option<String> },

    /// `@directive(args)` — Blade-style directive.
    Directive { name: String, args: Option<String> },

    /// `<x-name attr="val">` — component open.
    ComponentOpen {
        name: String,
        attrs: Vec<(String, String)>,
        self_closing: bool,
    },

    /// `</x-name>` — component close.
    ComponentClose { name: String },
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Text(s) => write!(f, "{s}"),
            Token::EscapedExpr(e) => write!(f, "{{{{ {e} }}}}"),
            Token::RawExpr(e) => write!(f, "{{!! {e} !!}}"),
            Token::LiteralExpr(e) => write!(f, "@{{{{ {e} }}}}"),
            Token::LiteralDirective {
                name,
                args: Some(a),
            } => write!(f, "@@{name}({a})"),
            Token::LiteralDirective { name, args: None } => write!(f, "@@{name}"),
            Token::Directive {
                name,
                args: Some(a),
            } => write!(f, "@{name}({a})"),
            Token::Directive { name, args: None } => write!(f, "@{name}"),
            Token::ComponentOpen {
                name,
                attrs,
                self_closing,
            } => {
                write!(f, "<x-{name}")?;
                for (k, v) in attrs {
                    write!(f, " {k}=\"{v}\"")?;
                }
                if *self_closing {
                    write!(f, " />")
                } else {
                    write!(f, ">")
                }
            }
            Token::ComponentClose { name } => write!(f, "</x-{name}>"),
        }
    }
}

pub fn tokenize(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0;
    let mut text_start = 0;

    while i < bytes.len() {
        // @{{ ... }} — Blade escape, output `{{ ... }}` literally.
        // Must come before the regular `{{ ... }}` rule so the `@` consumes
        // the brace pair instead of letting it kick into interpolation.
        if i + 2 < bytes.len() && bytes[i] == b'@' && bytes[i + 1] == b'{' && bytes[i + 2] == b'{' {
            flush_text(input, text_start, i, &mut tokens);
            if let Some(end) = find_close(&input[i + 3..], "}}") {
                let expr = input[i + 3..i + 3 + end].trim().to_string();
                tokens.push(Token::LiteralExpr(expr));
                i += 3 + end + 2;
                text_start = i;
                continue;
            }
        }

        // @@directive — Blade escape, output `@directive` literally.
        if i + 1 < bytes.len()
            && bytes[i] == b'@'
            && bytes[i + 1] == b'@'
            && i + 2 < bytes.len()
            && (bytes[i + 2].is_ascii_alphabetic() || bytes[i + 2] == b'_')
        {
            flush_text(input, text_start, i, &mut tokens);
            let dir_start = i + 2;
            let mut dir_end = dir_start;
            while dir_end < bytes.len()
                && (bytes[dir_end].is_ascii_alphanumeric() || bytes[dir_end] == b'_')
            {
                dir_end += 1;
            }
            let name = input[dir_start..dir_end].to_string();
            let mut args = None;
            let mut new_i = dir_end;
            if dir_end < bytes.len() && bytes[dir_end] == b'(' {
                if let Some(close_offset) = find_matching_paren(&input[dir_end..]) {
                    args = Some(input[dir_end + 1..dir_end + close_offset].to_string());
                    new_i = dir_end + close_offset + 1;
                }
            }
            tokens.push(Token::LiteralDirective { name, args });
            i = new_i;
            text_start = i;
            continue;
        }

        // {{ ... }}
        if i + 1 < bytes.len()
            && bytes[i] == b'{'
            && bytes[i + 1] == b'{'
            && !(i + 2 < bytes.len() && bytes[i + 2] == b'-'/* askama escape */)
        {
            flush_text(input, text_start, i, &mut tokens);
            if let Some(end) = find_close(&input[i + 2..], "}}") {
                let expr = input[i + 2..i + 2 + end].trim().to_string();
                tokens.push(Token::EscapedExpr(expr));
                i += 2 + end + 2;
                text_start = i;
                continue;
            }
        }

        // {!! ... !!}
        if i + 2 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'!' && bytes[i + 2] == b'!' {
            flush_text(input, text_start, i, &mut tokens);
            if let Some(end) = find_close(&input[i + 3..], "!!}") {
                let expr = input[i + 3..i + 3 + end].trim().to_string();
                tokens.push(Token::RawExpr(expr));
                i += 3 + end + 3;
                text_start = i;
                continue;
            }
        }

        // @directive
        if bytes[i] == b'@'
            && i + 1 < bytes.len()
            && (bytes[i + 1].is_ascii_alphabetic() || bytes[i + 1] == b'_')
        {
            flush_text(input, text_start, i, &mut tokens);
            let dir_start = i + 1;
            let mut dir_end = dir_start;
            while dir_end < bytes.len()
                && (bytes[dir_end].is_ascii_alphanumeric() || bytes[dir_end] == b'_')
            {
                dir_end += 1;
            }
            let name = input[dir_start..dir_end].to_string();
            let mut args = None;
            let mut new_i = dir_end;
            if dir_end < bytes.len() && bytes[dir_end] == b'(' {
                if let Some(close_offset) = find_matching_paren(&input[dir_end..]) {
                    args = Some(input[dir_end + 1..dir_end + close_offset].to_string());
                    new_i = dir_end + close_offset + 1;
                }
            }
            tokens.push(Token::Directive { name, args });
            i = new_i;
            text_start = i;
            continue;
        }

        // <x-component ...>
        if bytes[i] == b'<' && i + 2 < bytes.len() && bytes[i + 1] == b'x' && bytes[i + 2] == b'-' {
            flush_text(input, text_start, i, &mut tokens);
            let after = &input[i + 3..];
            // Component name
            let name_end = after
                .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
                .unwrap_or(after.len());
            let name = after[..name_end].to_string();
            let rest_start = i + 3 + name_end;
            let close_offset = input[rest_start..]
                .find('>')
                .unwrap_or(input.len() - rest_start);
            let tag_inner = &input[rest_start..rest_start + close_offset];
            let self_closing = tag_inner.ends_with('/');
            let attrs = parse_attrs(tag_inner.trim_end_matches('/'));
            tokens.push(Token::ComponentOpen {
                name,
                attrs,
                self_closing,
            });
            i = rest_start + close_offset + 1;
            text_start = i;
            continue;
        }

        // </x-component>
        if bytes[i] == b'<'
            && i + 3 < bytes.len()
            && bytes[i + 1] == b'/'
            && bytes[i + 2] == b'x'
            && bytes[i + 3] == b'-'
        {
            flush_text(input, text_start, i, &mut tokens);
            let after = &input[i + 4..];
            let name_end = after.find('>').unwrap_or(after.len());
            let name = after[..name_end].trim().to_string();
            tokens.push(Token::ComponentClose { name });
            i += 4 + name_end + 1;
            text_start = i;
            continue;
        }

        i += 1;
    }

    flush_text(input, text_start, bytes.len(), &mut tokens);
    tokens
}

fn flush_text(input: &str, start: usize, end: usize, tokens: &mut Vec<Token>) {
    if end > start {
        tokens.push(Token::Text(input[start..end].to_string()));
    }
}

fn find_close(s: &str, needle: &str) -> Option<usize> {
    s.find(needle)
}

fn find_matching_paren(s: &str) -> Option<usize> {
    // s starts with '('. Returns offset of matching ')'.
    let bytes = s.as_bytes();
    if bytes.is_empty() || bytes[0] != b'(' {
        return None;
    }
    let mut depth = 1;
    let mut in_string = None::<u8>;
    for (i, &b) in bytes.iter().enumerate().skip(1) {
        if let Some(quote) = in_string {
            if b == quote && bytes.get(i - 1) != Some(&b'\\') {
                in_string = None;
            }
            continue;
        }
        match b {
            b'"' | b'\'' => in_string = Some(b),
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_attrs(s: &str) -> Vec<(String, String)> {
    let mut attrs = Vec::new();
    let mut chars = s.char_indices().peekable();
    while let Some((_, ch)) = chars.peek() {
        if ch.is_whitespace() {
            chars.next();
            continue;
        }
        // Read attribute name
        let mut name_end = 0;
        let mut name = String::new();
        let mut found_eq = false;
        while let Some(&(idx, c)) = chars.peek() {
            if c == '=' {
                found_eq = true;
                name_end = idx;
                chars.next();
                break;
            }
            if c.is_whitespace() {
                name_end = idx;
                break;
            }
            name.push(c);
            chars.next();
        }
        let _ = name_end;
        if !found_eq {
            attrs.push((name, String::new()));
            continue;
        }
        // Read value: quoted or unquoted
        if let Some(&(_, q)) = chars.peek() {
            if q == '"' || q == '\'' {
                chars.next();
                let mut val = String::new();
                while let Some(&(_, c)) = chars.peek() {
                    chars.next();
                    if c == q {
                        break;
                    }
                    val.push(c);
                }
                attrs.push((name, val));
                continue;
            }
        }
        // Unquoted value (read to whitespace)
        let mut val = String::new();
        while let Some(&(_, c)) = chars.peek() {
            if c.is_whitespace() {
                break;
            }
            val.push(c);
            chars.next();
        }
        attrs.push((name, val));
    }
    attrs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first_non_text(tokens: &[Token]) -> &Token {
        tokens
            .iter()
            .find(|t| !matches!(t, Token::Text(s) if s.trim().is_empty()))
            .expect("no non-empty token")
    }

    #[test]
    fn at_double_brace_emits_literal_expr_not_escaped() {
        let toks = tokenize("@{{ handle }}");
        assert!(
            matches!(first_non_text(&toks), Token::LiteralExpr(s) if s == "handle"),
            "got: {toks:?}"
        );
    }

    #[test]
    fn double_at_directive_emits_literal_directive() {
        let toks = tokenize("@@if(user)");
        let first = first_non_text(&toks);
        assert!(
            matches!(first, Token::LiteralDirective { name, args }
                if name == "if" && args.as_deref() == Some("user")),
            "got: {toks:?}"
        );
    }

    #[test]
    fn double_at_directive_without_args() {
        let toks = tokenize("@@verbatim");
        let first = first_non_text(&toks);
        assert!(
            matches!(first, Token::LiteralDirective { name, args }
                if name == "verbatim" && args.is_none()),
            "got: {toks:?}"
        );
    }

    #[test]
    fn at_double_brace_does_not_consume_following_real_interpolation() {
        // The user's reported case: `@{{ '' }}{{ handle }}` should produce
        // a literal `{{ '' }}` followed by interpolation of `handle`.
        let toks = tokenize("@{{ '' }}{{ handle }}");
        let mut iter = toks
            .iter()
            .filter(|t| !matches!(t, Token::Text(s) if s.trim().is_empty()));
        let first = iter.next().expect("first");
        let second = iter.next().expect("second");
        assert!(
            matches!(first, Token::LiteralExpr(s) if s == "''"),
            "first: {first:?}"
        );
        assert!(
            matches!(second, Token::EscapedExpr(s) if s == "handle"),
            "second: {second:?}"
        );
    }

    #[test]
    fn regular_directive_still_works() {
        // Sanity: `@if(x)` shouldn't be mis-categorized by the @@ rule.
        let toks = tokenize("@if(x)");
        let first = first_non_text(&toks);
        assert!(
            matches!(first, Token::Directive { name, args }
                if name == "if" && args.as_deref() == Some("x")),
            "got: {toks:?}"
        );
    }

    #[test]
    fn regular_interpolation_still_works() {
        // Sanity: `{{ name }}` (no leading @) is still escaped output.
        let toks = tokenize("{{ name }}");
        let first = first_non_text(&toks);
        assert!(
            matches!(first, Token::EscapedExpr(s) if s == "name"),
            "got: {toks:?}"
        );
    }
}
