//! Headless Chromium driver for browser-level integration tests.
//!
//! Enabled with the `browser` feature: `anvilforge-test = { version = ..., features = ["browser"] }`.
//! Requires a Chrome/Chromium binary on the host (chromiumoxide will auto-detect
//! one on PATH or honor `CHROME`).
//!
//! ```ignore
//! use anvilforge::assay::*;
//! use anvil_test::browser::Browser;
//!
//! #[tokio::test]
//! async fn counter_increments_in_browser() {
//!     let url = test_server::serve_blog().await;
//!     let page = Browser::launch().await.unwrap()
//!         .visit(&format!("{url}/spark-demo")).await.unwrap();
//!
//!     page.click("button[spark\\:click=\"increment\"]").await.unwrap();
//!     page.wait_for(".count, [data-test=\"count\"]").await.unwrap();
//!
//!     let count = page.text(".count, [data-test=\"count\"]").await.unwrap();
//!     expect(count.as_str()).to_be("1");
//! }
//! ```
//!
//! The driver is intentionally a thin, ergonomic wrapper around chromiumoxide.
//! For raw access, call `.page()` to drop down to the underlying `chromiumoxide::Page`.

use std::sync::Arc;

use chromiumoxide::browser::{Browser as RawBrowser, BrowserConfig};
use chromiumoxide::page::Page as RawPage;
use futures::StreamExt;
use tokio::sync::Mutex;

/// Lazy-launched, shared headless Chromium instance.
#[derive(Clone)]
pub struct Browser {
    inner: Arc<Mutex<RawBrowser>>,
}

impl Browser {
    /// Spin up a fresh headless Chromium. Returns an error if no browser binary
    /// can be located.
    pub async fn launch() -> Result<Self, String> {
        let config = BrowserConfig::builder()
            .build()
            .map_err(|e| format!("browser config: {e}"))?;
        let (browser, mut handler) = RawBrowser::launch(config)
            .await
            .map_err(|e| format!("launch chromium: {e}. Install Chrome/Chromium or set the CHROME env var to its path."))?;

        tokio::spawn(async move {
            while let Some(_evt) = handler.next().await {}
        });

        Ok(Self {
            inner: Arc::new(Mutex::new(browser)),
        })
    }

    /// Navigate to `url` and return a `Page` ready to be acted on.
    pub async fn visit(&self, url: &str) -> Result<Page, String> {
        let browser = self.inner.lock().await;
        let raw = browser
            .new_page(url)
            .await
            .map_err(|e| format!("new_page({url}): {e}"))?;
        raw.wait_for_navigation()
            .await
            .map_err(|e| format!("wait_for_navigation: {e}"))?;
        Ok(Page { raw })
    }
}

/// A live page. Drops the underlying tab when it goes out of scope.
pub struct Page {
    raw: RawPage,
}

impl Page {
    /// Click the first element matching `selector`.
    pub async fn click(&self, selector: &str) -> Result<&Self, String> {
        let el = self
            .raw
            .find_element(selector)
            .await
            .map_err(|e| format!("find_element({selector}): {e}"))?;
        el.click()
            .await
            .map_err(|e| format!("click({selector}): {e}"))?;
        Ok(self)
    }

    /// Fill an input by selector. Fires `input` + `change` events so
    /// frameworks (Spark, React, Vue) see the update.
    pub async fn fill(&self, selector: &str, value: &str) -> Result<&Self, String> {
        let escaped = serde_json::to_string(value).unwrap_or_else(|_| "\"\"".into());
        let sel_lit = serde_json::to_string(selector).unwrap_or_default();
        let script = format!(
            "(function() {{ const el = document.querySelector({sel_lit}); if (!el) return 'not_found'; el.value = {escaped}; el.dispatchEvent(new Event('input', {{ bubbles: true }})); el.dispatchEvent(new Event('change', {{ bubbles: true }})); return 'ok'; }})()"
        );
        let out = self
            .raw
            .evaluate(script)
            .await
            .map_err(|e| format!("evaluate(fill): {e}"))?;
        if let Some(v) = out.into_value::<String>().ok() {
            if v == "not_found" {
                return Err(format!("selector `{selector}` not found"));
            }
        }
        Ok(self)
    }

    /// Type into a focused element. Triggers keydown/keypress/keyup for each char.
    pub async fn type_into(&self, selector: &str, text: &str) -> Result<&Self, String> {
        let el = self
            .raw
            .find_element(selector)
            .await
            .map_err(|e| format!("find_element({selector}): {e}"))?;
        el.focus()
            .await
            .map_err(|e| format!("focus: {e}"))?;
        el.type_str(text)
            .await
            .map_err(|e| format!("type: {e}"))?;
        Ok(self)
    }

    /// Wait for `selector` to appear in the DOM, up to `timeout_ms`.
    pub async fn wait_for(&self, selector: &str) -> Result<&Self, String> {
        self.wait_for_with(selector, 5_000, 100).await
    }

    pub async fn wait_for_with(
        &self,
        selector: &str,
        timeout_ms: u64,
        poll_ms: u64,
    ) -> Result<&Self, String> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
        while std::time::Instant::now() < deadline {
            if self.raw.find_element(selector).await.is_ok() {
                return Ok(self);
            }
            tokio::time::sleep(std::time::Duration::from_millis(poll_ms)).await;
        }
        Err(format!("timed out waiting for `{selector}`"))
    }

    /// Get the text content of the first matching element.
    pub async fn text(&self, selector: &str) -> Result<String, String> {
        let el = self
            .raw
            .find_element(selector)
            .await
            .map_err(|e| format!("find_element({selector}): {e}"))?;
        let txt = el
            .inner_text()
            .await
            .map_err(|e| format!("inner_text: {e}"))?
            .unwrap_or_default();
        Ok(txt)
    }

    /// Get the value of an input by selector.
    pub async fn value(&self, selector: &str) -> Result<String, String> {
        let sel_lit = serde_json::to_string(selector).unwrap_or_default();
        let script = format!(
            "(function() {{ const el = document.querySelector({sel_lit}); return el ? el.value : null; }})()"
        );
        let out = self
            .raw
            .evaluate(script)
            .await
            .map_err(|e| format!("evaluate(value): {e}"))?;
        Ok(out.into_value::<String>().unwrap_or_default())
    }

    /// Evaluate arbitrary JS and return the result deserialized into `T`.
    pub async fn eval<T: serde::de::DeserializeOwned>(&self, script: &str) -> Result<T, String> {
        let wrapped = format!(
            "(function() {{ const __r = (function(){{ {script} }})(); return typeof __r === 'object' && __r !== null ? JSON.parse(JSON.stringify(__r)) : __r; }})()"
        );
        let out = self
            .raw
            .evaluate(wrapped)
            .await
            .map_err(|e| format!("eval: {e}"))?;
        out.into_value::<T>().map_err(|e| format!("eval decode: {e}"))
    }

    /// Take a PNG screenshot of the current page.
    pub async fn screenshot(&self) -> Result<Vec<u8>, String> {
        use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
        use chromiumoxide::page::ScreenshotParams;
        self.raw
            .screenshot(
                ScreenshotParams::builder()
                    .format(CaptureScreenshotFormat::Png)
                    .build(),
            )
            .await
            .map_err(|e| format!("screenshot: {e}"))
    }

    /// Current URL.
    pub async fn url(&self) -> String {
        self.raw.url().await.ok().flatten().unwrap_or_default()
    }

    /// Escape hatch — raw chromiumoxide page.
    pub fn raw(&self) -> &RawPage {
        &self.raw
    }
}

impl Drop for Page {
    fn drop(&mut self) {
        // best-effort tab close
        let page = self.raw.clone();
        tokio::spawn(async move {
            let _ = page.close().await;
        });
    }
}
