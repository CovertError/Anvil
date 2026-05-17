//! Lower a Forge token stream into Askama template syntax.

use crate::parser::Token;

pub fn lower(tokens: &[Token]) -> String {
    let mut out = String::new();
    let mut stack_pushes: Vec<String> = Vec::new(); // names of currently-open @push blocks

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
                let args_stripped_quotes =
                    args_inner.trim_matches(|c| c == '"' || c == '\'');

                match name.as_str() {
                    // Control flow
                    "if" => {
                        out.push_str("{% if ");
                        out.push_str(args_inner);
                        out.push_str(" %}");
                    }
                    "elseif" => {
                        out.push_str("{% elif ");
                        out.push_str(args_inner);
                        out.push_str(" %}");
                    }
                    "else" => out.push_str("{% else %}"),
                    "endif" => out.push_str("{% endif %}"),

                    "unless" => {
                        out.push_str("{% if !(");
                        out.push_str(args_inner);
                        out.push_str(") %}");
                    }
                    "endunless" => out.push_str("{% endif %}"),

                    "foreach" => {
                        // @foreach($items as $item)  →  {% for item in items %}
                        let (lhs, rhs) = split_foreach(args_inner);
                        out.push_str("{% for ");
                        out.push_str(&rhs);
                        out.push_str(" in ");
                        out.push_str(&lhs);
                        out.push_str(" %}");
                    }
                    "endforeach" => out.push_str("{% endfor %}"),

                    "for" => {
                        out.push_str("{% for ");
                        out.push_str(args_inner);
                        out.push_str(" %}");
                    }
                    "endfor" => out.push_str("{% endfor %}"),

                    "while" => {
                        // Askama doesn't have @while; lower to a comment so build doesn't crash.
                        out.push_str("{# @while not supported, use @foreach: ");
                        out.push_str(args_inner);
                        out.push_str(" #}");
                    }
                    "endwhile" => {}

                    // Layout
                    "extends" => {
                        out.push_str("{% extends \"");
                        out.push_str(&path_for(args_stripped_quotes));
                        out.push_str("\" %}");
                    }
                    "section" => {
                        out.push_str("{% block ");
                        out.push_str(args_stripped_quotes);
                        out.push_str(" %}");
                    }
                    "endsection" => out.push_str("{% endblock %}"),
                    "yield" => {
                        out.push_str("{% block ");
                        out.push_str(args_stripped_quotes);
                        out.push_str(" %}{% endblock %}");
                    }
                    "parent" => out.push_str("{{ super() }}"),
                    "include" => {
                        out.push_str("{% include \"");
                        out.push_str(&path_for(args_stripped_quotes));
                        out.push_str("\" %}");
                    }
                    "includeIf" | "includeif" => {
                        // simplistic: just include
                        out.push_str("{% include \"");
                        out.push_str(&path_for(args_stripped_quotes));
                        out.push_str("\" %}");
                    }

                    // Stacks
                    "stack" => {
                        // Emit placeholder. forge::stack::postprocess fills it.
                        out.push_str("<!--FORGE-STACK:");
                        out.push_str(args_stripped_quotes);
                        out.push_str("-->");
                    }
                    "push" => {
                        stack_pushes.push(args_stripped_quotes.to_string());
                        // Capture into a sub-block whose rendered content is sent to stack buffer.
                        // We do this by wrapping the content in a fake block and using a marker
                        // pre/post that the postprocess pass extracts.
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

                    // Vite
                    "vite" => {
                        // @vite(['resources/css/app.css', 'resources/js/app.js']) → call helper
                        out.push_str("{{ ::forge::vite::render(&[");
                        out.push_str(args_inner);
                        out.push_str("])|safe }}");
                    }

                    // Auth / Can — sugar over @if
                    "auth" => {
                        out.push_str("{% if auth_user.is_some() %}");
                    }
                    "endauth" => out.push_str("{% endif %}"),
                    "guest" => out.push_str("{% if auth_user.is_none() %}"),
                    "endguest" => out.push_str("{% endif %}"),
                    "can" => {
                        out.push_str("{% if can(");
                        out.push_str(args_inner);
                        out.push_str(") %}");
                    }
                    "endcan" => out.push_str("{% endif %}"),

                    // Verbatim is a no-op in Askama (no PHP-like raw mode required)
                    "verbatim" | "endverbatim" => {}

                    // CSRF token helper
                    "csrf" => {
                        out.push_str(
                            "<input type=\"hidden\" name=\"_token\" value=\"{{ csrf_token }}\">",
                        );
                    }

                    // Method spoofing for forms
                    "method" => {
                        out.push_str("<input type=\"hidden\" name=\"_method\" value=\"");
                        out.push_str(args_stripped_quotes);
                        out.push_str("\">");
                    }

                    // Unknown directive — emit a passthrough comment so build doesn't crash
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

fn path_for(spec: &str) -> String {
    // `layouts.app` → `layouts/app.html`
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
    // Forms:
    // "$items as $item" → ("items", "item")
    // "items as item" → ("items", "item")
    // "$users as $key => $user" → currently unsupported; collapse to items/users
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
