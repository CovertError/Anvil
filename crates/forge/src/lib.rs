//! Forge runtime: stack buffer for `@push`/`@stack`, `@vite` asset helper, escape primitives.

pub mod escape;
pub mod stack;
pub mod vite;

pub use askama;
pub use askama::Template;

/// Re-export the common Forge-style template builder.
pub trait View: askama::Template {
    fn render_view(&self) -> Result<String, askama::Error> {
        let raw = self.render()?;
        Ok(stack::postprocess(&raw))
    }
}

impl<T: askama::Template> View for T {}
