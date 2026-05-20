//! Compile-time-embedded static assets — the runtime hook for the
//! `embed-assets` feature.
//!
//! Disk-served `public/` mounts continue to work via `tower_http::ServeDir`.
//! When an app wants to ship as a single executable, it derives a
//! `rust_embed::RustEmbed` struct on its `public/` folder and registers a
//! fetcher here; `server::mount_static` then consults this registry before
//! falling back to disk.
//!
//! Registration is global because the user's `RustEmbed` type lives in their
//! crate, not in the framework, and we need a way to bridge across that
//! boundary without leaking generics through every config struct.

use std::borrow::Cow;
use std::collections::HashMap;

use once_cell::sync::OnceCell;
use parking_lot::RwLock;

/// A single embedded file's payload + metadata. Mirrors what `rust_embed::File`
/// exposes, but kept framework-owned so the runtime API stays stable if we
/// later swap the embedder.
pub struct EmbeddedAsset {
    pub data: Cow<'static, [u8]>,
    /// MIME type. Caller is expected to pre-resolve this (e.g. via
    /// `mime_guess::from_path`) so the framework doesn't have to guess.
    pub content_type: String,
    /// Optional strong validator. When present, the framework emits an `ETag`
    /// header and short-circuits matching `If-None-Match` requests with 304.
    pub etag: Option<String>,
    /// Optional Unix-seconds last-modified timestamp from the embedder.
    pub last_modified: Option<u64>,
}

/// Function pointer signature that backs an embedded mount. Takes a path
/// relative to the mount root (`foo/bar.css`, no leading slash) and returns
/// the file if it exists in the embedded set.
pub type EmbeddedAssetFetcher = fn(&str) -> Option<EmbeddedAsset>;

static REGISTRY: OnceCell<RwLock<HashMap<String, EmbeddedAssetFetcher>>> = OnceCell::new();

fn registry() -> &'static RwLock<HashMap<String, EmbeddedAssetFetcher>> {
    REGISTRY.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Register an embedded-asset fetcher for the given URL prefix (e.g. `"/assets"`).
/// Call this from app bootstrap before `serve()` runs. Re-registering the same
/// prefix replaces the previous fetcher — last writer wins.
pub fn register(prefix: impl Into<String>, fetcher: EmbeddedAssetFetcher) {
    let mut map = registry().write();
    map.insert(prefix.into(), fetcher);
}

/// Look up the fetcher for a mount prefix. Returns `None` when the mount has
/// no embedded backing and the caller should fall through to disk serving.
pub fn lookup(prefix: &str) -> Option<EmbeddedAssetFetcher> {
    registry().read().get(prefix).copied()
}

/// Resolve a file path's MIME type via `mime_guess`, defaulting to
/// `application/octet-stream` when the extension is unknown. Provided as a
/// convenience for fetcher implementations.
pub fn guess_mime(path: &str) -> String {
    mime_guess::from_path(path)
        .first_or_octet_stream()
        .essence_str()
        .to_string()
}

/// Pull an asset out of a `rust_embed::RustEmbed` impl and wrap it as an
/// `EmbeddedAsset`. The user crate's generated wrapper just delegates here so
/// they don't have to learn the `RustEmbed::get` shape themselves.
#[cfg(feature = "embed-assets")]
pub fn fetcher_from<E: rust_embed::RustEmbed + ?Sized>(path: &str) -> Option<EmbeddedAsset> {
    let file = E::get(path)?;
    let etag = file
        .metadata
        .sha256_hash()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    Some(EmbeddedAsset {
        content_type: guess_mime(path),
        etag: Some(etag),
        last_modified: file.metadata.last_modified(),
        data: file.data,
    })
}

/// One-liner for app authors: derive a `RustEmbed` struct on a folder and
/// register it as the backing store for a URL mount. Expands to a struct +
/// fetcher + a `register()` fn the bootstrap calls.
///
/// ```ignore
/// // src/embedded_assets.rs
/// anvil_core::embed_static!(PublicAssets, "/assets", "public");
///
/// // src/main.rs (inside bootstrap):
/// embedded_assets::register();
/// ```
#[cfg(feature = "embed-assets")]
#[macro_export]
macro_rules! embed_static {
    ($struct_name:ident, $prefix:expr, $folder:expr) => {
        #[derive($crate::rust_embed::RustEmbed)]
        #[folder = $folder]
        pub struct $struct_name;

        pub fn fetcher(path: &str) -> ::core::option::Option<$crate::embedded::EmbeddedAsset> {
            $crate::embedded::fetcher_from::<$struct_name>(path)
        }

        pub fn register() {
            $crate::embedded::register($prefix, fetcher);
        }
    };
}
