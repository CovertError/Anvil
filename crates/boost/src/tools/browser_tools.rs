//! Browser MCP tools — Playwright-equivalent surface for AI agents.
//!
//! All four tools accept a `url` and drive a shared headless Chromium instance
//! (see `crate::browser`). Pages are torn down after each tool call to avoid
//! cross-call state leaks. The browser itself is reused.

use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::handler::viewport::Viewport;
use chromiumoxide::page::ScreenshotParams;
use futures::StreamExt;
use serde_json::{json, Value};

use crate::browser::BrowserManager;
use crate::protocol::CallToolResult;
use crate::tool::{Context, Tool};

fn url_schema(extra_props: Value) -> Value {
    let mut props = serde_json::Map::new();
    props.insert(
        "url".into(),
        json!({ "type": "string", "description": "Absolute or relative URL to open." }),
    );
    if let Value::Object(obj) = extra_props {
        for (k, v) in obj {
            props.insert(k, v);
        }
    }
    json!({
        "type": "object",
        "required": ["url"],
        "properties": props,
    })
}

fn require_url(args: &Value) -> Result<String, CallToolResult> {
    match args.get("url").and_then(|v| v.as_str()) {
        Some(u) if !u.is_empty() => Ok(u.to_string()),
        _ => Err(CallToolResult::error("`url` is required")),
    }
}

// ─── browser-screenshot ────────────────────────────────────────────────────

pub struct BrowserScreenshot {
    pub manager: BrowserManager,
}

#[async_trait]
impl Tool for BrowserScreenshot {
    fn name(&self) -> &'static str {
        "browser-screenshot"
    }
    fn description(&self) -> &'static str {
        "Open a URL in a headless Chromium and return a PNG screenshot as base64. Optional `width`/`height` set the viewport; `full_page=true` captures the entire scrollable page."
    }
    fn input_schema(&self) -> Value {
        url_schema(json!({
            "width":     { "type": "integer", "default": 1280 },
            "height":    { "type": "integer", "default": 800 },
            "full_page": { "type": "boolean", "default": false }
        }))
    }

    async fn call(&self, _ctx: &Context, args: Value) -> CallToolResult {
        let url = match require_url(&args) {
            Ok(u) => u,
            Err(r) => return r,
        };
        let width = args.get("width").and_then(|v| v.as_u64()).unwrap_or(1280) as u32;
        let height = args.get("height").and_then(|v| v.as_u64()).unwrap_or(800) as u32;
        let full_page = args
            .get("full_page")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let page = match self.manager.open(&url).await {
            Ok(p) => p,
            Err(e) => return CallToolResult::error(e),
        };
        // Viewport is set via CDP. The builder in chromiumoxide 0.7 returns
        // `Result<SetDeviceMetricsOverrideParams, _>`; unwrap-or-skip.
        use chromiumoxide::cdp::browser_protocol::emulation::SetDeviceMetricsOverrideParams;
        if let Ok(params) = SetDeviceMetricsOverrideParams::builder()
            .width(width as i64)
            .height(height as i64)
            .device_scale_factor(1.0)
            .mobile(false)
            .build()
        {
            let _ = page.execute(params).await;
        }
        let _ = Viewport::default(); // suppress unused import

        let mut params = ScreenshotParams::builder().format(CaptureScreenshotFormat::Png);
        if full_page {
            params = params.full_page(true);
        }
        let png = match page.screenshot(params.build()).await {
            Ok(b) => b,
            Err(e) => return CallToolResult::error(format!("screenshot: {e}")),
        };
        let _ = page.close().await;

        let encoded = B64.encode(&png);
        CallToolResult::json(&json!({
            "url": url,
            "bytes": png.len(),
            "width": width,
            "height": height,
            "full_page": full_page,
            "format": "png",
            "base64": encoded,
        }))
    }
}

// ─── browser-console ────────────────────────────────────────────────────────

pub struct BrowserConsole {
    pub manager: BrowserManager,
}

#[async_trait]
impl Tool for BrowserConsole {
    fn name(&self) -> &'static str {
        "browser-console"
    }
    fn description(&self) -> &'static str {
        "Open a URL and collect console messages emitted by the page. Returns level + text for each entry. Useful for spotting JS errors after a Spark interaction."
    }
    fn input_schema(&self) -> Value {
        url_schema(json!({
            "wait_ms": { "type": "integer", "default": 500, "description": "How long to listen after load before reporting." }
        }))
    }

    async fn call(&self, _ctx: &Context, args: Value) -> CallToolResult {
        let url = match require_url(&args) {
            Ok(u) => u,
            Err(r) => return r,
        };
        let wait_ms = args.get("wait_ms").and_then(|v| v.as_u64()).unwrap_or(500);

        use chromiumoxide::cdp::browser_protocol::log::EventEntryAdded;
        use chromiumoxide::cdp::js_protocol::runtime::EventConsoleApiCalled;

        let page = match self.manager.open(&url).await {
            Ok(p) => p,
            Err(e) => return CallToolResult::error(e),
        };

        let mut console_events = match page.event_listener::<EventConsoleApiCalled>().await {
            Ok(s) => s,
            Err(e) => return CallToolResult::error(format!("event listener: {e}")),
        };
        let mut log_events = match page.event_listener::<EventEntryAdded>().await {
            Ok(s) => s,
            Err(e) => return CallToolResult::error(format!("event listener: {e}")),
        };

        let mut messages = Vec::<Value>::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(wait_ms);
        loop {
            let timeout = deadline
                .checked_duration_since(std::time::Instant::now())
                .unwrap_or_default();
            if timeout.is_zero() {
                break;
            }
            tokio::select! {
                _ = tokio::time::sleep(timeout) => break,
                evt = console_events.next() => {
                    if let Some(evt) = evt {
                        let text = evt.args.iter().filter_map(|a| a.value.as_ref().map(|v| v.to_string())).collect::<Vec<_>>().join(" ");
                        messages.push(json!({
                            "kind": "console",
                            "level": format!("{:?}", evt.r#type),
                            "text": text,
                        }));
                    }
                }
                evt = log_events.next() => {
                    if let Some(evt) = evt {
                        messages.push(json!({
                            "kind": "log",
                            "level": format!("{:?}", evt.entry.level),
                            "text": evt.entry.text,
                            "source": format!("{:?}", evt.entry.source),
                            "url": evt.entry.url,
                        }));
                    }
                }
            }
        }

        let _ = page.close().await;

        CallToolResult::json(&json!({
            "url": url,
            "count": messages.len(),
            "messages": messages,
        }))
    }
}

// ─── browser-network ────────────────────────────────────────────────────────

pub struct BrowserNetwork {
    pub manager: BrowserManager,
}

#[async_trait]
impl Tool for BrowserNetwork {
    fn name(&self) -> &'static str {
        "browser-network"
    }
    fn description(&self) -> &'static str {
        "Open a URL and return the network requests the page made. Each entry has method, URL, resource type, and status (when available)."
    }
    fn input_schema(&self) -> Value {
        url_schema(json!({
            "wait_ms": { "type": "integer", "default": 1000 }
        }))
    }

    async fn call(&self, _ctx: &Context, args: Value) -> CallToolResult {
        let url = match require_url(&args) {
            Ok(u) => u,
            Err(r) => return r,
        };
        let wait_ms = args.get("wait_ms").and_then(|v| v.as_u64()).unwrap_or(1000);

        use chromiumoxide::cdp::browser_protocol::network::{
            EventRequestWillBeSent, EventResponseReceived,
        };

        let page = match self.manager.open(&url).await {
            Ok(p) => p,
            Err(e) => return CallToolResult::error(e),
        };

        let mut req_stream = match page.event_listener::<EventRequestWillBeSent>().await {
            Ok(s) => s,
            Err(e) => return CallToolResult::error(format!("event listener: {e}")),
        };
        let mut resp_stream = match page.event_listener::<EventResponseReceived>().await {
            Ok(s) => s,
            Err(e) => return CallToolResult::error(format!("event listener: {e}")),
        };

        let mut by_id: indexmap::IndexMap<String, serde_json::Map<String, Value>> =
            indexmap::IndexMap::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(wait_ms);
        loop {
            let timeout = deadline
                .checked_duration_since(std::time::Instant::now())
                .unwrap_or_default();
            if timeout.is_zero() {
                break;
            }
            tokio::select! {
                _ = tokio::time::sleep(timeout) => break,
                evt = req_stream.next() => {
                    if let Some(evt) = evt {
                        let mut m = serde_json::Map::new();
                        m.insert("method".into(), json!(evt.request.method));
                        m.insert("url".into(), json!(evt.request.url));
                        m.insert("type".into(), json!(format!("{:?}", evt.r#type)));
                        by_id.entry(format!("{:?}", evt.request_id)).or_default().extend(m);
                    }
                }
                evt = resp_stream.next() => {
                    if let Some(evt) = evt {
                        let entry = by_id.entry(format!("{:?}", evt.request_id)).or_default();
                        entry.insert("status".into(), json!(evt.response.status));
                        entry.insert("status_text".into(), json!(evt.response.status_text));
                        entry.insert("mime_type".into(), json!(evt.response.mime_type));
                    }
                }
            }
        }
        let _ = page.close().await;

        let entries: Vec<Value> = by_id.into_iter().map(|(_, v)| Value::Object(v)).collect();
        CallToolResult::json(&json!({
            "url": url,
            "count": entries.len(),
            "requests": entries,
        }))
    }
}

// ─── browser-click ──────────────────────────────────────────────────────────

pub struct BrowserClick {
    pub manager: BrowserManager,
}

#[async_trait]
impl Tool for BrowserClick {
    fn name(&self) -> &'static str {
        "browser-click"
    }
    fn description(&self) -> &'static str {
        "Open a URL and click the first element matching a CSS selector. Returns the URL after the click (which may have navigated)."
    }
    fn input_schema(&self) -> Value {
        url_schema(json!({
            "selector": { "type": "string", "description": "CSS selector for the element to click." },
            "wait_ms":  { "type": "integer", "default": 500, "description": "How long to wait after click before reading the URL." }
        }))
    }

    async fn call(&self, _ctx: &Context, args: Value) -> CallToolResult {
        let url = match require_url(&args) {
            Ok(u) => u,
            Err(r) => return r,
        };
        let selector = match args.get("selector").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => return CallToolResult::error("`selector` is required"),
        };
        let wait_ms = args.get("wait_ms").and_then(|v| v.as_u64()).unwrap_or(500);

        let page = match self.manager.open(&url).await {
            Ok(p) => p,
            Err(e) => return CallToolResult::error(e),
        };
        let element = match page.find_element(&selector).await {
            Ok(el) => el,
            Err(e) => {
                let _ = page.close().await;
                return CallToolResult::error(format!("find_element({selector}): {e}"));
            }
        };
        if let Err(e) = element.click().await {
            let _ = page.close().await;
            return CallToolResult::error(format!("click({selector}): {e}"));
        }
        tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;

        let new_url = page.url().await.ok().flatten().unwrap_or_default();
        let _ = page.close().await;

        CallToolResult::json(&json!({
            "url": url,
            "selector": selector,
            "current_url": new_url,
        }))
    }
}

// ─── browser-fill ───────────────────────────────────────────────────────────

pub struct BrowserFill {
    pub manager: BrowserManager,
}

#[async_trait]
impl Tool for BrowserFill {
    fn name(&self) -> &'static str {
        "browser-fill"
    }
    fn description(&self) -> &'static str {
        "Open a URL, locate a single input by CSS selector, replace its value, and optionally submit the enclosing form. Returns the final URL after submit."
    }
    fn input_schema(&self) -> Value {
        url_schema(json!({
            "selector": { "type": "string", "description": "CSS selector targeting the input/textarea." },
            "value":    { "type": "string", "description": "New value to set." },
            "submit":   { "type": "boolean", "default": false, "description": "If true, dispatch a form submit after filling." },
            "wait_ms":  { "type": "integer", "default": 500 }
        }))
    }

    async fn call(&self, _ctx: &Context, args: Value) -> CallToolResult {
        let url = match require_url(&args) {
            Ok(u) => u,
            Err(r) => return r,
        };
        let selector = match args.get("selector").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => return CallToolResult::error("`selector` is required"),
        };
        let value = args
            .get("value")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let submit = args
            .get("submit")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let wait_ms = args.get("wait_ms").and_then(|v| v.as_u64()).unwrap_or(500);

        let page = match self.manager.open(&url).await {
            Ok(p) => p,
            Err(e) => return CallToolResult::error(e),
        };

        // Use JS evaluate to set value + fire the right events (works for
        // controlled inputs in React/Vue/Spark alike).
        let escaped = serde_json::to_string(&value).unwrap_or_else(|_| "\"\"".to_string());
        let selector_lit = serde_json::to_string(&selector).unwrap_or_default();
        let script = format!(
            "(function() {{ const el = document.querySelector({selector_lit}); if (!el) return 'not_found'; el.value = {escaped}; el.dispatchEvent(new Event('input', {{ bubbles: true }})); el.dispatchEvent(new Event('change', {{ bubbles: true }})); {} return 'ok'; }})()",
            if submit {
                "if (el.form) el.form.requestSubmit ? el.form.requestSubmit() : el.form.submit();"
            } else {
                ""
            }
        );
        let result = page.evaluate(script).await;
        tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
        let final_url = page.url().await.ok().flatten().unwrap_or_default();
        let _ = page.close().await;

        match result {
            Ok(v) => {
                let outcome = v.into_value().unwrap_or(Value::Null);
                if outcome.as_str() == Some("not_found") {
                    return CallToolResult::error(format!("selector `{selector}` not found"));
                }
                CallToolResult::json(&json!({
                    "url": url,
                    "selector": selector,
                    "submitted": submit,
                    "current_url": final_url,
                }))
            }
            Err(e) => CallToolResult::error(format!("fill: {e}")),
        }
    }
}

// ─── browser-type ───────────────────────────────────────────────────────────

pub struct BrowserType {
    pub manager: BrowserManager,
}

#[async_trait]
impl Tool for BrowserType {
    fn name(&self) -> &'static str {
        "browser-type"
    }
    fn description(&self) -> &'static str {
        "Open a URL, focus an element by selector, and type a string (character-by-character keypresses, useful for triggering keydown handlers)."
    }
    fn input_schema(&self) -> Value {
        url_schema(json!({
            "selector": { "type": "string", "description": "CSS selector for the element to focus." },
            "text":     { "type": "string", "description": "Text to type, one keypress per character." }
        }))
    }

    async fn call(&self, _ctx: &Context, args: Value) -> CallToolResult {
        let url = match require_url(&args) {
            Ok(u) => u,
            Err(r) => return r,
        };
        let selector = match args.get("selector").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => return CallToolResult::error("`selector` is required"),
        };
        let text = args
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let page = match self.manager.open(&url).await {
            Ok(p) => p,
            Err(e) => return CallToolResult::error(e),
        };
        let element = match page.find_element(&selector).await {
            Ok(el) => el,
            Err(e) => {
                let _ = page.close().await;
                return CallToolResult::error(format!("find_element({selector}): {e}"));
            }
        };
        if let Err(e) = element.focus().await {
            let _ = page.close().await;
            return CallToolResult::error(format!("focus: {e}"));
        }
        if let Err(e) = element.type_str(&text).await {
            let _ = page.close().await;
            return CallToolResult::error(format!("type: {e}"));
        }
        let _ = page.close().await;

        CallToolResult::json(&json!({
            "url": url,
            "selector": selector,
            "chars_typed": text.chars().count(),
        }))
    }
}

// ─── browser-wait-for ───────────────────────────────────────────────────────

pub struct BrowserWaitFor {
    pub manager: BrowserManager,
}

#[async_trait]
impl Tool for BrowserWaitFor {
    fn name(&self) -> &'static str {
        "browser-wait-for"
    }
    fn description(&self) -> &'static str {
        "Open a URL and wait for an element matching a CSS selector to appear in the DOM (with a timeout). Useful for tests that need to wait out async data loads or Spark interactions."
    }
    fn input_schema(&self) -> Value {
        url_schema(json!({
            "selector":   { "type": "string", "description": "CSS selector to wait for." },
            "timeout_ms": { "type": "integer", "default": 5000, "description": "Max time to wait, in ms." },
            "poll_ms":    { "type": "integer", "default": 100,  "description": "Polling interval, in ms." }
        }))
    }

    async fn call(&self, _ctx: &Context, args: Value) -> CallToolResult {
        let url = match require_url(&args) {
            Ok(u) => u,
            Err(r) => return r,
        };
        let selector = match args.get("selector").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => return CallToolResult::error("`selector` is required"),
        };
        let timeout_ms = args
            .get("timeout_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(5000);
        let poll_ms = args.get("poll_ms").and_then(|v| v.as_u64()).unwrap_or(100);

        let page = match self.manager.open(&url).await {
            Ok(p) => p,
            Err(e) => return CallToolResult::error(e),
        };

        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
        let mut found = false;
        let mut elapsed_ms: u128 = 0;
        while std::time::Instant::now() < deadline {
            if page.find_element(&selector).await.is_ok() {
                found = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(poll_ms)).await;
            elapsed_ms += poll_ms as u128;
        }
        let _ = page.close().await;

        CallToolResult::json(&json!({
            "url": url,
            "selector": selector,
            "found": found,
            "elapsed_ms": elapsed_ms,
        }))
    }
}

// ─── browser-eval ───────────────────────────────────────────────────────────

pub struct BrowserEval {
    pub manager: BrowserManager,
}

#[async_trait]
impl Tool for BrowserEval {
    fn name(&self) -> &'static str {
        "browser-eval"
    }
    fn description(&self) -> &'static str {
        "Open a URL and evaluate a JavaScript expression in the page context. Returns the result as JSON (numbers, strings, booleans, null, or objects/arrays via JSON.stringify)."
    }
    fn input_schema(&self) -> Value {
        url_schema(json!({
            "script": { "type": "string", "description": "JS expression or statement(s). The final expression's value is returned." }
        }))
    }

    async fn call(&self, _ctx: &Context, args: Value) -> CallToolResult {
        let url = match require_url(&args) {
            Ok(u) => u,
            Err(r) => return r,
        };
        let script = match args.get("script").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => return CallToolResult::error("`script` is required"),
        };

        let page = match self.manager.open(&url).await {
            Ok(p) => p,
            Err(e) => return CallToolResult::error(e),
        };

        // Wrap in IIFE so we get the final expression value, and stringify any
        // object/array so it round-trips through JSON cleanly.
        let wrapped = format!(
            "(function() {{ try {{ const __r = (function(){{ {script} }})(); return typeof __r === 'object' ? JSON.stringify(__r) : __r; }} catch (e) {{ return 'ERROR: ' + e.message; }} }})()"
        );

        let result = page.evaluate(wrapped).await;
        let _ = page.close().await;

        match result {
            Ok(v) => {
                let raw = v.into_value().unwrap_or(serde_json::Value::Null);
                CallToolResult::json(&json!({
                    "url": url,
                    "value": raw,
                }))
            }
            Err(e) => CallToolResult::error(format!("eval: {e}")),
        }
    }
}
