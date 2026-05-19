//! Forge runtime: stack buffer for `@push`/`@stack`, `@vite` asset helper, escape primitives.

pub mod escape;
pub mod helpers;
pub mod stack;
pub mod vite;

pub use askama;
pub use askama::Template;
pub use helpers::{class_list, lang, lang_choice, style_list, FormErrors, OldInput};

/// Re-export the common Forge-style template builder.
pub trait View: askama::Template {
    fn render_view(&self) -> Result<String, askama::Error> {
        let raw = self.render()?;
        Ok(stack::postprocess(&raw))
    }
}

impl<T: askama::Template> View for T {}
