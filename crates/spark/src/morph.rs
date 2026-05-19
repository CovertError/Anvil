//! Island slicing — extract `spark:island="name"` subtrees from rendered HTML so
//! the server can return only the changed region in `effects.islands`.
//!
//! The slicing is byte-accurate but naive: we scan for the opening attribute and
//! pair it with the matching close tag at the same nesting depth.

/// Find the inner HTML of the named `spark:island="..."` region. Returns
/// `Some(html_string)` if found, including the whole tag including its outer
/// attributes (so the JS runtime can morph the wrapper too).
pub fn slice_island(html: &str, island_name: &str) -> Option<String> {
    let needle = format!(r#"spark:island="{island_name}""#);
    let attr_pos = html.find(&needle)?;
    // Walk backwards to the opening `<` of this tag.
    let tag_open = html[..attr_pos].rfind('<')?;
    // Find tag name (e.g. "div") to enable balanced matching.
    let tag_name_start = tag_open + 1;
    let after_name = html[tag_name_start..]
        .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
        .unwrap_or(0);
    let tag_name = &html[tag_name_start..tag_name_start + after_name];
    // Close of the opening tag.
    let open_close = tag_open + html[tag_open..].find('>')?;
    let _ = open_close;
    // Find the matching close `</tag>` at the same depth.
    let close = find_balanced_close(html, tag_open, tag_name)?;
    Some(html[tag_open..close].to_string())
}

fn find_balanced_close(html: &str, start: usize, tag: &str) -> Option<usize> {
    let mut depth: i32 = 0;
    let open_marker = format!("<{tag}");
    let close_marker = format!("</{tag}>");
    let mut i = start;
    while i < html.len() {
        if html[i..].starts_with(&close_marker) {
            depth -= 1;
            if depth == 0 {
                return Some(i + close_marker.len());
            }
            i += close_marker.len();
            continue;
        }
        if html[i..].starts_with(&open_marker) {
            // Find the end of this opening tag (skip self-closing).
            if let Some(end_rel) = html[i..].find('>') {
                let self_close = html.as_bytes().get(i + end_rel - 1) == Some(&b'/');
                if !self_close {
                    depth += 1;
                }
                i += end_rel + 1;
                continue;
            }
            return None;
        }
        // Advance by one character.
        let next = html[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        i += next;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slices_named_island() {
        let html = r#"<div spark:id="x"><h1>head</h1><div spark:island="messages"><ul><li>hi</li></ul></div><p>footer</p></div>"#;
        let sliced = slice_island(html, "messages").unwrap();
        assert!(sliced.contains("<ul><li>hi</li></ul>"));
        assert!(sliced.starts_with("<div spark:island=\"messages\""));
        assert!(sliced.ends_with("</div>"));
    }

    #[test]
    fn returns_none_when_missing() {
        let html = "<div spark:id=\"x\"></div>";
        assert!(slice_island(html, "messages").is_none());
    }

    #[test]
    fn handles_nested_same_tag() {
        let html = r#"<div spark:island="a"><div>inner</div><div>also</div></div>"#;
        let sliced = slice_island(html, "a").unwrap();
        assert_eq!(
            sliced,
            r#"<div spark:island="a"><div>inner</div><div>also</div></div>"#
        );
    }
}
