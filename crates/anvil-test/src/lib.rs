//! Anvilforge test utilities — Pest-flavored HTTP and fluent expectations.
//!
//! Import the prelude in your test files:
//!
//! ```ignore
//! use anvilforge::assay::*;
//!
//! #[tokio::test]
//! async fn root_returns_welcome() {
//!     let client = TestClient::new(bootstrap::app::build().await.unwrap()).await;
//!
//!     client.get("/").await
//!         .assert_ok()
//!         .assert_see("Welcome");
//!
//!     expect(2 + 2).to_be(4);
//!     expect("hello world").to_contain("world");
//!     expect(vec![1, 2, 3]).to_have_length(3);
//! }
//! ```

pub mod client;
pub mod datasets;
pub mod expect;
pub mod factory;

#[cfg(feature = "browser")]
pub mod browser;

pub use client::{TestClient, TestResponse};
pub use expect::{expect, Expect, Not};
pub use factory::Factory;

// Re-exported for the `dataset!` macro's name-concatenation. Keeps user
// crates from needing to add `paste` as a direct dependency.
pub use paste;

/// The Pest-style prelude. `use anvilforge::assay::*;` or
/// `use anvil_test::assay::*;` to bring in the testing surface.
pub mod assay {
    pub use crate::{dataset, dataset_async};
    pub use crate::{expect, Expect, Factory, Not, TestClient, TestResponse};
    pub use serde_json::json;

    #[cfg(feature = "browser")]
    pub use crate::browser::{Browser, Page};
}
