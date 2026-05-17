//! View rendering helpers. Wraps Forge templates as HTTP responses.

use crate::response::ViewResponse;

/// Render an Askama-compatible template into a `ViewResponse`.
///
/// Forge templates compile down to Askama; this is the runtime entry point.
pub fn render<T>(template: &T) -> Result<ViewResponse, crate::Error>
where
    T: askama::Template,
{
    let body = template
        .render()
        .map_err(|e| crate::Error::Template(e.to_string()))?;
    let body = forge::stack::postprocess(&body);
    Ok(ViewResponse::new(body))
}

/// `view!` macro shortcut for handlers: returns a `Result<ViewResponse, Error>`.
#[macro_export]
macro_rules! view {
    ($template:expr) => {
        $crate::view::render(&$template)
    };
}
