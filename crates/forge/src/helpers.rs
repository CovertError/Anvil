//! Runtime helpers for Forge directives: `@class`, `@style`, `@lang`, `@old`.

/// Build a class string from a list of `(class, condition)` pairs.
///
/// ```ignore
/// // In a template:
/// @class([("active", is_active), ("muted", !is_active), ("bg-red-50", has_error)])
///
/// // Lowered to: forge::class_list(&[("active", is_active), ...])
/// ```
pub fn class_list(pairs: &[(&str, bool)]) -> String {
    let mut out = String::new();
    for (class, cond) in pairs {
        if *cond {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(class);
        }
    }
    out
}

/// Build an inline-style string from `(property, value, condition)` triples,
/// or just always-applied `(property, value)` pairs.
///
/// ```ignore
/// @style([("color", "red", has_error), ("font-weight", "bold", true)])
/// ```
pub fn style_list(triples: &[(&str, &str, bool)]) -> String {
    let mut out = String::new();
    for (prop, val, cond) in triples {
        if *cond {
            if !out.is_empty() {
                out.push_str("; ");
            }
            out.push_str(prop);
            out.push_str(": ");
            out.push_str(val);
        }
    }
    out
}

/// Placeholder for translation lookup. v0.2 will integrate `fluent` or `rust-i18n`.
///
/// For now, returns the key passed in — apps wanting i18n today can replace this
/// helper via the `forge` re-export.
pub fn lang(key: &str) -> String {
    key.to_string()
}

/// Placeholder for pluralized translation. v0.2 ships real plural rules.
pub fn lang_choice(key: &str, count: i64) -> String {
    if count == 1 {
        key.to_string()
    } else {
        format!("{key} (×{count})")
    }
}

/// A request-scoped map of old form input values, used by `@old('field')`
/// to repopulate forms after a validation failure + redirect.
///
/// The web stack writes flash data containing old input on validation failure;
/// the next request reads it into a `HashMap<String, String>` named `old_input`
/// that the template renders against.
pub type OldInput = std::collections::HashMap<String, String>;

/// A typed alias for the error map exposed to templates via `@error('field')`.
pub type FormErrors = ::indexmap::IndexMap<String, Vec<String>>;
