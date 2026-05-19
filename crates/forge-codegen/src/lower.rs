//! Lower a Forge token stream into Askama template syntax (the default) or
//! into a MiniJinja-compatible runtime form (for Spark component bodies and
//! other dynamic templates).

use crate::parser::Token;

/// Which template engine the lowered output targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LowerTarget {
    /// Compile-time Askama. Rust expressions inside directives are valid.
    #[default]
    Askama,
    /// Runtime MiniJinja. Spark directives lower to function calls that the
    /// runtime registers on the `Environment`; other Rust-flavored directives
    /// pass through unchanged (and will likely fail at render time if used).
    MiniJinja,
}

pub fn lower(tokens: &[Token]) -> String {
    lower_with_target(tokens, LowerTarget::Askama)
}

pub fn lower_with_target(tokens: &[Token], target: LowerTarget) -> String {
    let mut out = String::new();
    let mut stack_pushes: Vec<String> = Vec::new();
    let mut forelse_stack: Vec<String> = Vec::new(); // collection identifier per @forelse
    let mut switch_started = false; // whether we've emitted the first {% if %} for a @switch

    for tok in tokens {
        match tok {
            Token::Text(s) => out.push_str(s),

            Token::EscapedExpr(expr) => {
                out.push_str("{{ ");
                out.push_str(expr);
                out.push_str(" }}");
            }

            Token::RawExpr(expr) => {
                out.push_str("{{ ");
                out.push_str(expr);
                out.push_str("|safe }}");
            }

            Token::Directive { name, args } => {
                let args_inner = args.as_deref().unwrap_or("").trim();
                let args_stripped_quotes = args_inner.trim_matches(|c| c == '"' || c == '\'');

                match name.as_str() {
                    // ─── Control flow ──────────────────────────────────────
                    "if" => emit_if(&mut out, args_inner),
                    "elseif" => emit_elif(&mut out, args_inner),
                    "else" => out.push_str("{% else %}"),
                    "endif" => out.push_str("{% endif %}"),

                    "unless" => {
                        out.push_str("{% if !(");
                        out.push_str(args_inner);
                        out.push_str(") %}");
                    }
                    "endunless" => out.push_str("{% endif %}"),

                    "isset" => {
                        // Askama doesn't have isset; treat Option types via .is_some()
                        out.push_str("{% if (");
                        out.push_str(args_inner);
                        out.push_str(").is_some() %}");
                    }
                    "endisset" => out.push_str("{% endif %}"),

                    "empty" if !forelse_stack.is_empty() => {
                        // Inside @forelse: end the for and emit else branch.
                        out.push_str("{% endfor %}");
                        let _ = forelse_stack.pop();
                        out.push_str("{% if true %}");
                        // Track the @empty branch as a pseudo-if so @endforelse closes it.
                        forelse_stack.push("__empty_branch__".to_string());
                    }
                    "empty" => {
                        // Standalone @empty: shorthand for @if(value.is_empty())
                        out.push_str("{% if (");
                        out.push_str(args_inner);
                        out.push_str(").is_empty() %}");
                    }
                    "endempty" => out.push_str("{% endif %}"),

                    // ─── Loops ─────────────────────────────────────────────
                    "foreach" => {
                        let (lhs, rhs) = split_foreach(args_inner);
                        out.push_str("{% for ");
                        out.push_str(&rhs);
                        out.push_str(" in ");
                        out.push_str(&lhs);
                        out.push_str(" %}");
                    }
                    "endforeach" => out.push_str("{% endfor %}"),

                    "forelse" => {
                        let (lhs, rhs) = split_foreach(args_inner);
                        forelse_stack.push(lhs.clone());
                        out.push_str("{% for ");
                        out.push_str(&rhs);
                        out.push_str(" in ");
                        out.push_str(&lhs);
                        out.push_str(" %}");
                    }
                    "endforelse" => {
                        // Close whichever branch we're in.
                        if let Some(top) = forelse_stack.pop() {
                            if top == "__empty_branch__" {
                                out.push_str("{% endif %}");
                            } else {
                                out.push_str("{% endfor %}");
                            }
                        }
                    }

                    "for" => {
                        out.push_str("{% for ");
                        out.push_str(args_inner);
                        out.push_str(" %}");
                    }
                    "endfor" => out.push_str("{% endfor %}"),

                    "continue" => {
                        if args_inner.is_empty() {
                            out.push_str("{% continue %}");
                        } else {
                            out.push_str("{% if ");
                            out.push_str(args_inner);
                            out.push_str(" %}{% continue %}{% endif %}");
                        }
                    }
                    "break" => {
                        if args_inner.is_empty() {
                            out.push_str("{% break %}");
                        } else {
                            out.push_str("{% if ");
                            out.push_str(args_inner);
                            out.push_str(" %}{% break %}{% endif %}");
                        }
                    }

                    "while" => {
                        out.push_str("{# @while not supported, use @foreach: ");
                        out.push_str(args_inner);
                        out.push_str(" #}");
                    }
                    "endwhile" => {}

                    // ─── Switch ────────────────────────────────────────────
                    "switch" => {
                        out.push_str("{# @switch(");
                        out.push_str(args_inner);
                        out.push_str(") #}");
                        switch_started = false;
                        // Save the switch expression for case matching.
                        // We rely on the user repeating it in @case for clarity.
                        out.push_str("<!--SWITCH:");
                        out.push_str(args_inner);
                        out.push_str("-->");
                    }
                    "case" => {
                        // Translate @case(value) to either {% if switch_expr == value %} or {% elif ... %}
                        // We don't track the switch expr here; use a sentinel `__switch__` that downstream
                        // tooling can patch. For practical use, prefer @if/@elseif explicitly.
                        if !switch_started {
                            out.push_str("{% if __switch__ == ");
                            switch_started = true;
                        } else {
                            out.push_str("{% elif __switch__ == ");
                        }
                        out.push_str(args_inner);
                        out.push_str(" %}");
                    }
                    "default" => {
                        if switch_started {
                            out.push_str("{% else %}");
                        }
                    }
                    "endswitch" => {
                        if switch_started {
                            out.push_str("{% endif %}");
                            switch_started = false;
                        }
                    }

                    // ─── Layout / inheritance ──────────────────────────────
                    "extends" => {
                        out.push_str("{% extends \"");
                        out.push_str(&path_for(args_stripped_quotes));
                        out.push_str("\" %}");
                    }
                    "section" => {
                        // Support both block form (@section/@endsection) and inline (@section('title', 'My title'))
                        if let Some((name, value)) = parse_inline_section(args_inner) {
                            out.push_str("{% block ");
                            out.push_str(&name);
                            out.push_str(" %}");
                            out.push_str(&value);
                            out.push_str("{% endblock %}");
                        } else {
                            out.push_str("{% block ");
                            out.push_str(args_stripped_quotes);
                            out.push_str(" %}");
                        }
                    }
                    "endsection" | "show" | "stop" | "overwrite" => out.push_str("{% endblock %}"),
                    "yield" => {
                        // @yield('name') or @yield('name', 'default') — Askama doesn't have a one-liner
                        // default; use {% block %}default{% endblock %} so subtemplates can override.
                        let (n, d) = parse_yield_args(args_inner);
                        out.push_str("{% block ");
                        out.push_str(&n);
                        out.push_str(" %}");
                        out.push_str(&d);
                        out.push_str("{% endblock %}");
                    }
                    "parent" => out.push_str("{{ super() }}"),
                    "include" => {
                        out.push_str("{% include \"");
                        out.push_str(&path_for(args_stripped_quotes));
                        out.push_str("\" %}");
                    }
                    "includeIf" | "includeif" | "includeWhen" | "includewhen" => {
                        out.push_str("{% include \"");
                        out.push_str(&path_for(args_stripped_quotes));
                        out.push_str("\" %}");
                    }
                    "hasSection" | "hassection" => {
                        out.push_str("{% if true %}{# @hasSection placeholder for '");
                        out.push_str(args_stripped_quotes);
                        out.push_str("' #}");
                    }
                    "endhasSection" | "endhassection" | "sectionMissing" | "sectionmissing" => {
                        out.push_str("{% endif %}");
                    }

                    // ─── Stacks ────────────────────────────────────────────
                    "stack" => {
                        out.push_str("<!--FORGE-STACK:");
                        out.push_str(args_stripped_quotes);
                        out.push_str("-->");
                    }
                    "push" => {
                        stack_pushes.push(args_stripped_quotes.to_string());
                        out.push_str("<!--FORGE-PUSH-START:");
                        out.push_str(args_stripped_quotes);
                        out.push_str("-->");
                    }
                    "endpush" => {
                        let name = stack_pushes.pop().unwrap_or_default();
                        out.push_str("<!--FORGE-PUSH-END:");
                        out.push_str(&name);
                        out.push_str("-->");
                    }
                    "pushOnce" | "pushonce" => {
                        stack_pushes.push(args_stripped_quotes.to_string());
                        out.push_str("<!--FORGE-PUSHONCE-START:");
                        out.push_str(args_stripped_quotes);
                        out.push_str("-->");
                    }
                    "endPushOnce" | "endpushonce" => {
                        let name = stack_pushes.pop().unwrap_or_default();
                        out.push_str("<!--FORGE-PUSHONCE-END:");
                        out.push_str(&name);
                        out.push_str("-->");
                    }
                    "prepend" => {
                        stack_pushes.push(args_stripped_quotes.to_string());
                        out.push_str("<!--FORGE-PREPEND-START:");
                        out.push_str(args_stripped_quotes);
                        out.push_str("-->");
                    }
                    "endprepend" => {
                        let name = stack_pushes.pop().unwrap_or_default();
                        out.push_str("<!--FORGE-PREPEND-END:");
                        out.push_str(&name);
                        out.push_str("-->");
                    }
                    "once" => out.push_str("<!--FORGE-ONCE-START-->"),
                    "endonce" => out.push_str("<!--FORGE-ONCE-END-->"),

                    // ─── Assets ────────────────────────────────────────────
                    "vite" => {
                        out.push_str("{{ ::forge::vite::render(&[");
                        out.push_str(args_inner);
                        out.push_str("])|safe }}");
                    }

                    // ─── Auth / authorization ──────────────────────────────
                    "auth" => out.push_str("{% if auth_user.is_some() %}"),
                    "endauth" => out.push_str("{% endif %}"),
                    "guest" => out.push_str("{% if auth_user.is_none() %}"),
                    "endguest" => out.push_str("{% endif %}"),
                    "can" => {
                        out.push_str("{% if can(");
                        out.push_str(args_inner);
                        out.push_str(") %}");
                    }
                    "endcan" => out.push_str("{% endif %}"),
                    "cannot" => {
                        out.push_str("{% if !can(");
                        out.push_str(args_inner);
                        out.push_str(") %}");
                    }
                    "endcannot" => out.push_str("{% endif %}"),
                    "role" => {
                        out.push_str("{% if has_role(auth_user, ");
                        out.push_str(args_inner);
                        out.push_str(") %}");
                    }
                    "endrole" => out.push_str("{% endif %}"),

                    // ─── Form attribute helpers ────────────────────────────
                    "checked" => emit_attr_helper(&mut out, "checked", args_inner),
                    "selected" => emit_attr_helper(&mut out, "selected", args_inner),
                    "disabled" => emit_attr_helper(&mut out, "disabled", args_inner),
                    "readonly" => emit_attr_helper(&mut out, "readonly", args_inner),
                    "required" => emit_attr_helper(&mut out, "required", args_inner),

                    // ─── Validation feedback ───────────────────────────────
                    "error" => {
                        // @error('field') ... @enderror — exposes `message` inside the block.
                        out.push_str("{% if let Some(message) = errors.get(\"");
                        out.push_str(args_stripped_quotes);
                        out.push_str("\").and_then(|v| v.first()) %}");
                    }
                    "enderror" => out.push_str("{% endif %}"),
                    "old" => {
                        // @old('field', 'default') — renders the old input value via runtime helper.
                        let (field, default) = parse_old_args(args_inner);
                        out.push_str("{{ ::forge::escape::html(&old_input.get(\"");
                        out.push_str(&field);
                        out.push_str("\").cloned().unwrap_or_else(|| ");
                        out.push_str(&default);
                        out.push_str(".to_string())) }}");
                    }

                    // ─── Conditional class & style ─────────────────────────
                    "class" => {
                        // @class([("active", is_active), ("muted", !is_active)])
                        out.push_str("class=\"{{ ::forge::escape::html(&::forge::class_list(&[");
                        out.push_str(args_inner);
                        out.push_str("])) }}\"");
                    }
                    "style" => {
                        out.push_str("style=\"{{ ::forge::escape::html(&::forge::style_list(&[");
                        out.push_str(args_inner);
                        out.push_str("])) }}\"");
                    }

                    // ─── Debug helpers ─────────────────────────────────────
                    "dump" | "dd" => {
                        out.push_str("<pre>{{ format!(\"{:#?}\", ");
                        out.push_str(args_inner);
                        out.push_str(")|escape }}</pre>");
                    }
                    "json" => {
                        // @json($var) — emit as JSON inside <script> safely.
                        out.push_str("{{ ::serde_json::to_string(&");
                        out.push_str(args_inner);
                        out.push_str(").unwrap_or_default()|safe }}");
                    }

                    // ─── Verbatim ──────────────────────────────────────────
                    "verbatim" | "endverbatim" => {}

                    // ─── CSRF / method spoofing ────────────────────────────
                    "csrf" => {
                        out.push_str(
                            "<input type=\"hidden\" name=\"_token\" value=\"{{ csrf_token }}\">",
                        );
                    }
                    "method" => {
                        out.push_str("<input type=\"hidden\" name=\"_method\" value=\"");
                        out.push_str(args_stripped_quotes);
                        out.push_str("\">");
                    }

                    // ─── i18n placeholders (real impl in v0.2) ─────────────
                    "lang" | "trans" => {
                        out.push_str("{{ ::forge::escape::html(&::forge::lang(");
                        out.push_str(args_inner);
                        out.push_str(")) }}");
                    }
                    "choice" => {
                        out.push_str("{{ ::forge::escape::html(&::forge::lang_choice(");
                        out.push_str(args_inner);
                        out.push_str(")) }}");
                    }

                    // ─── Component prop declaration ────────────────────────
                    "props" => {
                        // @props([...]) — for now, emitted as a comment marker;
                        // real prop-default handling is done at lowering of component templates.
                        out.push_str("{# props: ");
                        out.push_str(args_inner);
                        out.push_str(" #}");
                    }

                    // ─── Spark (Livewire-equivalent) ───────────────────────
                    // @spark("counter", { initial: 5, label: "Clicks" })
                    "spark" => match target {
                        LowerTarget::Askama => {
                            let (name, props_expr) = parse_spark_args(args_inner);
                            out.push_str("{{ ::spark::render::render_mount(\"");
                            out.push_str(&name.replace('"', "\\\""));
                            out.push_str("\", &::spark::serde_json::json!(");
                            if props_expr.is_empty() {
                                out.push_str("null");
                            } else {
                                out.push_str(&props_expr);
                            }
                            out.push_str(")).unwrap_or_default()|safe }}");
                        }
                        LowerTarget::MiniJinja => {
                            // MiniJinja: emit a call to the `spark_mount` runtime
                            // function (registered on the Environment). Props
                            // dict-literal keys are auto-quoted so JS-style
                            // `{ initial: 5 }` becomes Jinja-style `{"initial": 5}`.
                            let (name, props_expr) = parse_spark_args(args_inner);
                            out.push_str("{{ spark_mount(\"");
                            out.push_str(&name.replace('"', "\\\""));
                            if props_expr.is_empty() {
                                out.push_str("\")|safe }}");
                            } else {
                                out.push_str("\", ");
                                out.push_str(&quote_dict_keys(&props_expr));
                                out.push_str(")|safe }}");
                            }
                        }
                    },

                    // @sparkScripts  — emits the runtime <script> + boot JSON.
                    "sparkScripts" | "sparkscripts" => match target {
                        LowerTarget::Askama => {
                            out.push_str("{{ ::spark::render::boot_script()|safe }}");
                        }
                        LowerTarget::MiniJinja => {
                            out.push_str("{{ spark_scripts()|safe }}");
                        }
                    },

                    // @sparkIsland("name") … @endSparkIsland — partial-render region.
                    "sparkIsland" | "sparkisland" => {
                        let name = args_stripped_quotes;
                        out.push_str("<div spark:island=\"");
                        out.push_str(name);
                        out.push_str("\">");
                    }
                    "endSparkIsland" | "endsparkisland" => {
                        out.push_str("</div>");
                    }

                    // ─── Fallback ──────────────────────────────────────────
                    _ => {
                        out.push_str("{# unknown directive @");
                        out.push_str(name);
                        if !args_inner.is_empty() {
                            out.push('(');
                            out.push_str(args_inner);
                            out.push(')');
                        }
                        out.push_str(" #}");
                    }
                }
            }

            Token::ComponentOpen {
                name,
                attrs,
                self_closing,
            } => {
                // Lower <x-alert type="error"> → {% call alert(type="error") %}
                let component_macro = component_macro_name(name);
                out.push_str("{% call ");
                out.push_str(&component_macro);
                out.push('(');
                let mut first = true;
                for (k, v) in attrs {
                    if !first {
                        out.push_str(", ");
                    }
                    first = false;
                    out.push_str(k);
                    out.push_str("=\"");
                    out.push_str(v);
                    out.push('"');
                }
                out.push_str(") %}");
                if *self_closing {
                    out.push_str("{% endcall %}");
                }
            }

            Token::ComponentClose { .. } => {
                out.push_str("{% endcall %}");
            }
        }
    }

    out
}

// ─── helpers ────────────────────────────────────────────────────────────────

fn emit_if(out: &mut String, expr: &str) {
    out.push_str("{% if ");
    out.push_str(expr);
    out.push_str(" %}");
}

fn emit_elif(out: &mut String, expr: &str) {
    out.push_str("{% elif ");
    out.push_str(expr);
    out.push_str(" %}");
}

fn emit_attr_helper(out: &mut String, attr: &str, condition: &str) {
    out.push_str("{% if (");
    out.push_str(condition);
    out.push_str(") %}");
    out.push_str(attr);
    out.push_str("{% endif %}");
}

fn parse_inline_section(args: &str) -> Option<(String, String)> {
    // @section('title', 'My title') → ("title", "My title")
    let args = args.trim();
    if let Some(comma) = find_top_level_comma(args) {
        let name = args[..comma]
            .trim()
            .trim_matches(|c| c == '"' || c == '\'')
            .to_string();
        let value = args[comma + 1..].trim().to_string();
        if !name.is_empty() {
            return Some((name, value));
        }
    }
    None
}

fn parse_yield_args(args: &str) -> (String, String) {
    if let Some(comma) = find_top_level_comma(args) {
        let n = args[..comma]
            .trim()
            .trim_matches(|c| c == '"' || c == '\'')
            .to_string();
        let d_raw = args[comma + 1..].trim();
        let d = if d_raw.starts_with('"') || d_raw.starts_with('\'') {
            // String literal default — emit as is.
            d_raw.trim_matches(|c| c == '"' || c == '\'').to_string()
        } else {
            // Expression default — wrap in {{ }}.
            format!("{{{{ {d_raw} }}}}")
        };
        return (n, d);
    }
    (
        args.trim()
            .trim_matches(|c| c == '"' || c == '\'')
            .to_string(),
        String::new(),
    )
}

fn parse_old_args(args: &str) -> (String, String) {
    if let Some(comma) = find_top_level_comma(args) {
        let f = args[..comma]
            .trim()
            .trim_matches(|c| c == '"' || c == '\'')
            .to_string();
        let d = args[comma + 1..].trim().to_string();
        return (f, d);
    }
    (
        args.trim()
            .trim_matches(|c| c == '"' || c == '\'')
            .to_string(),
        "\"\"".to_string(),
    )
}

/// Rewrite a JS-style dict literal so identifier-shaped keys are quoted,
/// producing output that both `serde_json::json!` (Askama path) and MiniJinja
/// can parse. `{ initial: 5, label: "x" }` → `{ "initial": 5, "label": "x" }`.
///
/// Skips substrings inside string literals so colons inside `"foo:bar"` aren't
/// misclassified as key separators.
fn quote_dict_keys(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len() + 16);
    let mut i = 0;
    let mut in_str: Option<u8> = None;

    while i < bytes.len() {
        let b = bytes[i];

        // String passthrough.
        if let Some(q) = in_str {
            out.push(b as char);
            if b == q && bytes.get(i.saturating_sub(1)) != Some(&b'\\') {
                in_str = None;
            }
            i += 1;
            continue;
        }
        if b == b'"' || b == b'\'' {
            in_str = Some(b);
            out.push(b as char);
            i += 1;
            continue;
        }

        // Look for an identifier-shaped key immediately after `{` or `,`.
        if b == b'{' || b == b',' {
            out.push(b as char);
            i += 1;
            // Skip whitespace.
            while i < bytes.len() && (bytes[i] as char).is_whitespace() {
                out.push(bytes[i] as char);
                i += 1;
            }
            // Identifier start?
            if i < bytes.len() && (bytes[i].is_ascii_alphabetic() || bytes[i] == b'_') {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                let ident = &input[start..i];
                // Peek for ':' (with optional whitespace) to confirm it's a key.
                let mut j = i;
                while j < bytes.len() && (bytes[j] as char).is_whitespace() {
                    j += 1;
                }
                if j < bytes.len() && bytes[j] == b':' {
                    out.push('"');
                    out.push_str(ident);
                    out.push('"');
                    // Skip captured whitespace (already in `out` from inside
                    // the identifier loop? no — we didn't push during the
                    // peek). Push the whitespace from i..j.
                    out.push_str(&input[i..j]);
                    out.push(':');
                    i = j + 1;
                    continue;
                } else {
                    // Not a key — pass the identifier through unchanged.
                    out.push_str(ident);
                    continue;
                }
            }
            continue;
        }

        out.push(b as char);
        i += 1;
    }
    out
}

/// Parse `@spark("name", { ... })` arguments.
///
/// Returns (component_name, props_expression). `props_expression` is the
/// untouched substring between the first comma and the end of args, which the
/// caller wraps in `serde_json::json!(...)`. Identifier-keyed object literals
/// (`{ initial: 5, label: "x" }`) work because `serde_json::json!` accepts them.
fn parse_spark_args(args: &str) -> (String, String) {
    let args = args.trim();
    if args.is_empty() {
        return (String::new(), String::new());
    }
    if let Some(comma) = find_top_level_comma(args) {
        let name = args[..comma]
            .trim()
            .trim_matches(|c| c == '"' || c == '\'')
            .to_string();
        let props = args[comma + 1..].trim().to_string();
        return (name, props);
    }
    (
        args.trim_matches(|c: char| c == '"' || c == '\'')
            .to_string(),
        String::new(),
    )
}

fn find_top_level_comma(s: &str) -> Option<usize> {
    let mut depth_paren = 0;
    let mut depth_bracket = 0;
    let mut in_string = None::<char>;
    for (i, ch) in s.char_indices() {
        if let Some(q) = in_string {
            if ch == q {
                in_string = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => in_string = Some(ch),
            '(' => depth_paren += 1,
            ')' => depth_paren -= 1,
            '[' => depth_bracket += 1,
            ']' => depth_bracket -= 1,
            ',' if depth_paren == 0 && depth_bracket == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

fn path_for(spec: &str) -> String {
    let s = spec.replace('.', "/");
    if s.ends_with(".html") {
        s
    } else {
        format!("{s}.html")
    }
}

fn component_macro_name(name: &str) -> String {
    name.replace('-', "_")
}

fn split_foreach(args: &str) -> (String, String) {
    let args = args.trim();
    if let Some((lhs, rhs)) = args.split_once(" as ") {
        let lhs = lhs.trim().trim_start_matches('$').to_string();
        let rhs = rhs.trim().trim_start_matches('$').to_string();
        if let Some((_k, v)) = rhs.split_once("=>") {
            return (lhs, v.trim().trim_start_matches('$').to_string());
        }
        (lhs, rhs)
    } else {
        (args.to_string(), "item".to_string())
    }
}
