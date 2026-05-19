//! Parse relationship attributes attached to a model: `#[has_many(Post)]`, etc.

use syn::DeriveInput;

#[derive(Debug)]
pub struct RelationDecl {
    pub kind: String,          // "HasMany" | "HasOne" | "BelongsTo"
    pub kind_token: syn::Path, // e.g. `cast_core::HasMany`
    pub method_name: String,   // snake_case method to generate (e.g. "posts")
    pub target: syn::Path,     // the related model's type path
    pub local_key: String,
    pub foreign_key: String,
}

pub fn collect_relations(input: &DeriveInput) -> syn::Result<Vec<RelationDecl>> {
    let mut relations = Vec::new();
    let struct_name = input.ident.to_string();
    let parent_snake = snake_case(&struct_name);
    let parent_pk = "id".to_string();

    for attr in &input.attrs {
        let name = attr
            .path()
            .get_ident()
            .map(|i| i.to_string())
            .unwrap_or_default();

        match name.as_str() {
            "has_many" => {
                let (target, fk_override, method_override) = parse_relation_args(attr)?;
                let target_ident = last_segment(&target);
                let method =
                    method_override.unwrap_or_else(|| pluralize_snake(&snake_case(&target_ident)));
                let foreign_key = fk_override.unwrap_or_else(|| format!("{}_id", &parent_snake));
                relations.push(RelationDecl {
                    kind: "HasMany".into(),
                    kind_token: syn::parse_quote!(HasMany),
                    method_name: method,
                    target,
                    local_key: parent_pk.clone(),
                    foreign_key,
                });
            }
            "has_one" => {
                let (target, fk_override, method_override) = parse_relation_args(attr)?;
                let target_ident = last_segment(&target);
                let method = method_override.unwrap_or_else(|| snake_case(&target_ident));
                let foreign_key = fk_override.unwrap_or_else(|| format!("{}_id", &parent_snake));
                relations.push(RelationDecl {
                    kind: "HasOne".into(),
                    kind_token: syn::parse_quote!(HasOne),
                    method_name: method,
                    target,
                    local_key: parent_pk.clone(),
                    foreign_key,
                });
            }
            "belongs_to" => {
                let (target, fk_override, method_override) = parse_relation_args(attr)?;
                let target_ident = last_segment(&target);
                let method = method_override.unwrap_or_else(|| snake_case(&target_ident));
                let foreign_key =
                    fk_override.unwrap_or_else(|| format!("{}_id", snake_case(&target_ident)));
                relations.push(RelationDecl {
                    kind: "BelongsTo".into(),
                    kind_token: syn::parse_quote!(BelongsTo),
                    method_name: method,
                    target,
                    local_key: parent_pk.clone(),
                    foreign_key,
                });
            }
            _ => {}
        }
    }
    Ok(relations)
}

fn parse_relation_args(
    attr: &syn::Attribute,
) -> syn::Result<(syn::Path, Option<String>, Option<String>)> {
    // Accept: `#[has_many(Post)]` or `#[has_many(Post, foreign_key = "author_id", as = "articles")]`
    let mut target: Option<syn::Path> = None;
    let mut fk: Option<String> = None;
    let mut method: Option<String> = None;

    attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("foreign_key") {
            let v: syn::LitStr = meta.value()?.parse()?;
            fk = Some(v.value());
            return Ok(());
        }
        if meta.path.is_ident("as") {
            let v: syn::LitStr = meta.value()?.parse()?;
            method = Some(v.value());
            return Ok(());
        }
        if target.is_none() {
            target = Some(meta.path.clone());
            return Ok(());
        }
        Err(meta.error("unexpected argument"))
    })?;

    let target =
        target.ok_or_else(|| syn::Error::new_spanned(attr, "missing related model type"))?;
    Ok((target, fk, method))
}

fn last_segment(path: &syn::Path) -> String {
    path.segments
        .last()
        .map(|s| s.ident.to_string())
        .unwrap_or_default()
}

fn snake_case(s: &str) -> String {
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(ch.to_ascii_lowercase());
    }
    out
}

fn pluralize_snake(s: &str) -> String {
    if s.ends_with('s') {
        s.to_string()
    } else if s.ends_with('y') {
        let mut s = s.to_string();
        s.pop();
        s.push_str("ies");
        s
    } else {
        format!("{s}s")
    }
}
