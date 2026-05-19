//! Lazy shared Chromium instance used by the browser MCP tools.
//!
//! The browser is launched on first use and kept alive for the lifetime of the
//! Boost server process. We hold its `Handler` future in a detached background
//! task so DevTools events keep flowing while we drive pages from the tools.
//!
//! Failure mode: if Chromium isn't installed and `chromiumoxide` can't find one
//! on PATH, every call returns a clean MCP error explaining how to fix it.
//! Tools never panic.

use std::sync::Arc;

use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::page::Page;
use futures::StreamExt;
use once_cell::sync::OnceCell;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct BrowserManager {
    inner: Arc<Inner>,
}

struct Inner {
    cell: OnceCell<Arc<Mutex<Browser>>>,
    init: Mutex<()>,
}

impl Default for BrowserManager {
    fn default() -> Self {
        Self::new()
    }
}

impl BrowserManager {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                cell: OnceCell::new(),
                init: Mutex::new(()),
            }),
        }
    }

    /// Return a handle to the singleton browser, launching it on first call.
    pub async fn browser(&self) -> Result<Arc<Mutex<Browser>>, String> {
        if let Some(b) = self.inner.cell.get() {
            return Ok(b.clone());
        }
        let _g = self.inner.init.lock().await;
        if let Some(b) = self.inner.cell.get() {
            return Ok(b.clone());
        }
        let config = BrowserConfig::builder()
            .build()
            .map_err(|e| format!("browser config: {e}"))?;
        let (browser, mut handler) = Browser::launch(config)
            .await
            .map_err(|e| format!("launch chromium: {e}. Install Chrome/Chromium or set CHROME env var to its path."))?;

        // Drain DevTools events forever so pages remain responsive.
        tokio::spawn(async move {
            while let Some(_evt) = handler.next().await {
                // Ignore — keeping the channel alive is enough.
            }
        });

        let arc = Arc::new(Mutex::new(browser));
        let _ = self.inner.cell.set(arc.clone());
        Ok(arc)
    }

    /// Open `url`, wait for `load`, and return the live page.
    pub async fn open(&self, url: &str) -> Result<Page, String> {
        let browser_arc = self.browser().await?;
        let browser = browser_arc.lock().await;
        let page = browser
            .new_page(url)
            .await
            .map_err(|e| format!("new_page({url}): {e}"))?;
        page.wait_for_navigation()
            .await
            .map_err(|e| format!("wait_for_navigation: {e}"))?;
        Ok(page)
    }
}
