//! Production HTTP serving configuration — the NGINX-equivalent surface.
//!
//! Apps can load this from `config/anvil.toml` via `ServerConfig::from_file`,
//! or build it programmatically via the typed structs. Env vars override file
//! values where applicable (Laravel-style precedence).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    /// Bind address. Default: `127.0.0.1:8080` (set in `from_env`).
    #[serde(default = "default_bind")]
    pub bind: String,

    /// Virtual host names this server answers to. Empty = match all hosts.
    /// Supports wildcard prefixes: `"*.example.com"` matches any subdomain.
    #[serde(default)]
    pub server_name: Vec<String>,

    /// Optional TLS config. If present, the server runs HTTPS.
    pub tls: Option<TlsConfig>,

    /// Optional HTTP-to-HTTPS auto-redirect listener. Typically binds :80 and
    /// 301-redirects every request to the equivalent `https://` URL.
    pub redirect_http: Option<RedirectHttpConfig>,

    /// HTTP Strict Transport Security (HSTS) header. Off by default.
    #[serde(default)]
    pub hsts: HstsConfig,

    /// Body/timeout limits.
    #[serde(default)]
    pub limits: LimitsConfig,

    /// Compression layer config.
    #[serde(default)]
    pub compression: CompressionConfig,

    /// Static file mounts — map of URL prefix → on-disk dir + cache policy.
    #[serde(default)]
    pub static_files: BTreeMap<String, StaticMount>,

    /// Rate limiting rules.
    #[serde(default)]
    pub rate_limit: RateLimitConfig,

    /// Trusted reverse-proxy ranges. Forwarded headers from outside these
    /// CIDRs are ignored.
    #[serde(default)]
    pub trusted_proxies: TrustedProxiesConfig,

    /// Access log config.
    #[serde(default)]
    pub access_log: AccessLogConfig,

    /// URL rewrite rules (regex `from` → `to`, optionally as a redirect).
    #[serde(default)]
    pub rewrites: Vec<RewriteRule>,

    /// Custom error pages — map of status code (as a string key) → file path.
    #[serde(default)]
    pub error_pages: BTreeMap<String, std::path::PathBuf>,

    /// Trailing-slash policy.
    #[serde(default)]
    pub trailing_slash: TrailingSlashConfig,

    /// Reverse-proxy rules — path prefix → upstream URL.
    #[serde(default, rename = "proxy")]
    pub proxies: Vec<ProxyRule>,

    /// CORS configuration.
    #[serde(default)]
    pub cors: CorsConfig,

    /// Path-prefixed IP allow/deny rules.
    #[serde(default)]
    pub ip_rules: Vec<IpRule>,

    /// Path-prefixed HTTP Basic Auth blocks.
    #[serde(default, rename = "basic_auth")]
    pub basic_auth: Vec<BasicAuthRule>,
}

fn default_bind() -> String {
    "127.0.0.1:8080".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsConfig {
    pub cert: PathBuf,
    pub key: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedirectHttpConfig {
    /// Plain-HTTP listener address (typically `"0.0.0.0:80"`).
    pub bind: String,

    /// 301 (permanent) when `true`, 302 (temporary) when `false`. Default: 301.
    #[serde(default = "yes")]
    pub permanent: bool,

    /// Target host for the redirect. If unset, the request's Host header is
    /// reused (with the scheme flipped to `https`).
    pub target_host: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HstsConfig {
    pub enabled: bool,

    /// `max-age=<seconds>`. Defaults to `1y` when HSTS is enabled.
    #[serde(deserialize_with = "deserialize_opt_duration", default)]
    pub max_age: Option<Duration>,

    pub include_subdomains: bool,
    pub preload: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LimitsConfig {
    /// Max request body size. Accepts `"10MB"`, `"500KB"`, `"2GB"`, raw byte count.
    #[serde(deserialize_with = "deserialize_size", default = "default_body_max")]
    pub body_max: u64,

    /// Per-request timeout for the handler. `None` = no timeout.
    #[serde(deserialize_with = "deserialize_opt_duration", default)]
    pub request_timeout: Option<Duration>,
}

impl Default for LimitsConfig {
    fn default() -> Self {
        Self {
            body_max: default_body_max(),
            request_timeout: None,
        }
    }
}

fn default_body_max() -> u64 {
    16 * 1024 * 1024 // 16MB
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CompressionConfig {
    /// Enable compression. Off by default — flip via config or env.
    pub enabled: bool,

    /// Algorithms to advertise via `Accept-Encoding` matching. Order matters.
    /// Accepts `"gzip"`, `"br"`, `"deflate"`.
    pub algorithms: Vec<String>,

    /// Minimum response size (bytes) below which compression is skipped.
    /// Accepts `"1KB"`, raw bytes.
    #[serde(deserialize_with = "deserialize_size", default = "default_min_size")]
    pub min_size: u64,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            algorithms: vec!["gzip".to_string()],
            min_size: default_min_size(),
        }
    }
}

fn default_min_size() -> u64 {
    1024
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StaticMount {
    /// On-disk directory served at this URL prefix.
    pub dir: PathBuf,

    /// `Cache-Control: max-age=<seconds>` value. Accepts `"1y"`, `"30d"`, `"3600"`.
    /// Default: no Cache-Control header is set.
    #[serde(deserialize_with = "deserialize_opt_duration", default)]
    pub cache: Option<Duration>,

    /// Whether to enable byte-range requests (default: true).
    #[serde(default = "yes")]
    pub ranges: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RateLimitConfig {
    /// Default per-IP rate (e.g. `"60/minute"`). `None` disables the default rate.
    pub per_ip: Option<String>,

    /// Per-route overrides: `{"POST /login" = "5/minute"}`.
    #[serde(default)]
    pub routes: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TrustedProxiesConfig {
    /// CIDR ranges from which X-Forwarded-* headers will be honored.
    pub ranges: Vec<ipnet::IpNet>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RewriteRule {
    /// Regex applied to the request path (or full path+query, when `match_query` is true).
    pub from: String,

    /// Replacement template. Capture groups available as `$1`, `$2`, etc.
    pub to: String,

    /// HTTP status to return. `301`/`302`/`307`/`308` send a redirect. Any other
    /// value (or unset) does an in-place internal rewrite — the request URI is
    /// rewritten before reaching the handler.
    #[serde(default)]
    pub status: Option<u16>,

    /// If true, the regex is applied to `path?query` instead of just `path`.
    #[serde(default)]
    pub match_query: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TrailingSlashConfig {
    /// `"always"` — append `/` to paths missing one (redirect or rewrite).
    /// `"never"` — strip trailing `/`.
    /// `"ignore"` (default) — leave alone.
    pub mode: TrailingSlashMode,

    /// `"redirect"` (default) returns a 301; `"rewrite"` modifies the URI in place.
    pub action: TrailingSlashAction,
}

impl Default for TrailingSlashConfig {
    fn default() -> Self {
        Self {
            mode: TrailingSlashMode::Ignore,
            action: TrailingSlashAction::Redirect,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TrailingSlashMode {
    Always,
    Never,
    Ignore,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TrailingSlashAction {
    Redirect,
    Rewrite,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CorsConfig {
    pub enabled: bool,
    /// Allowed origins. `["*"]` allows any. Default: empty.
    pub allow_origins: Vec<String>,
    /// Allowed methods. Default: `["GET", "POST", "OPTIONS"]` when enabled.
    pub allow_methods: Vec<String>,
    /// Allowed headers. Default: a reasonable set when enabled.
    pub allow_headers: Vec<String>,
    /// Expose these response headers to the JS layer.
    pub expose_headers: Vec<String>,
    /// Whether credentials (cookies, auth headers) are allowed cross-origin.
    pub allow_credentials: bool,
    /// `Access-Control-Max-Age` for preflight cache.
    #[serde(deserialize_with = "deserialize_opt_duration", default)]
    pub max_age: Option<Duration>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IpRule {
    /// Path prefix this rule applies to.
    pub prefix: String,
    /// `"allow"` or `"deny"`.
    pub action: IpAction,
    /// CIDR ranges (or single IPs) covered by this rule.
    pub ranges: Vec<ipnet::IpNet>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IpAction {
    Allow,
    Deny,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BasicAuthRule {
    pub prefix: String,
    /// `realm` shown in the browser's auth prompt.
    #[serde(default = "default_realm")]
    pub realm: String,
    /// Inline credentials as `user:password` pairs.
    pub credentials: Vec<String>,
}

fn default_realm() -> String {
    "Restricted".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProxyRule {
    /// Path prefix that triggers this proxy (e.g. `"/api/v2"`).
    pub prefix: String,

    /// Upstream base URL (e.g. `"http://api-v2.internal:8080"`).
    pub upstream: String,

    /// Strip the prefix from the request path before forwarding. Default: false.
    #[serde(default)]
    pub strip_prefix: bool,

    /// Keep the original Host header instead of using the upstream host. Default: false.
    #[serde(default)]
    pub preserve_host: bool,

    /// Per-request timeout. Defaults to 30s.
    #[serde(deserialize_with = "deserialize_opt_duration", default)]
    pub timeout: Option<Duration>,

    /// How many times to retry on connection failure. Default: 0.
    #[serde(default)]
    pub retries: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AccessLogConfig {
    pub format: AccessLogFormat,
    pub path: Option<PathBuf>,
}

impl Default for AccessLogConfig {
    fn default() -> Self {
        Self {
            format: AccessLogFormat::Combined,
            path: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AccessLogFormat {
    /// Apache "combined" format: `host - - [time] "method path proto" status bytes`
    Combined,
    /// Newline-delimited JSON, one object per request.
    Json,
    /// Off — only the framework's TraceLayer fires.
    Off,
}

impl ServerConfig {
    /// Load from `config/anvil.toml` if present, otherwise return defaults.
    pub fn from_file_or_default(path: impl AsRef<std::path::Path>) -> Self {
        match Self::from_file(path.as_ref()) {
            Ok(c) => c,
            Err(crate::Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => {
                tracing::warn!(?e, path = %path.as_ref().display(), "failed to load server config; using defaults");
                Self::default()
            }
        }
    }

    pub fn from_file(path: &std::path::Path) -> crate::Result<Self> {
        let bytes = std::fs::read_to_string(path)?;
        let cfg: Self = toml::from_str(&bytes)
            .map_err(|e| crate::Error::Config(format!("toml parse {}: {e}", path.display())))?;
        Ok(cfg.apply_env_overrides())
    }

    /// Apply env-var overrides for the most common keys, mirroring Laravel's
    /// `config(...)` + `.env` precedence.
    pub fn apply_env_overrides(mut self) -> Self {
        if let Ok(v) = std::env::var("APP_ADDR") {
            self.bind = v;
        }
        if let (Ok(cert), Ok(key)) = (std::env::var("TLS_CERT"), std::env::var("TLS_KEY")) {
            self.tls = Some(TlsConfig {
                cert: PathBuf::from(cert),
                key: PathBuf::from(key),
            });
        }
        self
    }
}

// ─── Helpers: parse human-readable sizes / durations ────────────────────────

fn deserialize_size<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    use serde::de::Error;
    let v = toml::Value::deserialize(d)?;
    match v {
        toml::Value::Integer(n) => Ok(n.max(0) as u64),
        toml::Value::String(s) => parse_size(&s).map_err(D::Error::custom),
        other => Err(D::Error::custom(format!(
            "expected integer or size string, got {other:?}"
        ))),
    }
}

fn deserialize_opt_duration<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Duration>, D::Error> {
    use serde::de::Error;
    let v = Option::<toml::Value>::deserialize(d)?;
    match v {
        None | Some(toml::Value::String(_)) if matches!(&v, Some(toml::Value::String(s)) if s.is_empty()) => {
            Ok(None)
        }
        None => Ok(None),
        Some(toml::Value::Integer(n)) => Ok(Some(Duration::from_secs(n.max(0) as u64))),
        Some(toml::Value::String(s)) => parse_duration(&s).map(Some).map_err(D::Error::custom),
        Some(other) => Err(D::Error::custom(format!(
            "expected integer (seconds) or duration string, got {other:?}"
        ))),
    }
}

/// Parse `"10MB"`, `"500KB"`, `"2GB"`, or a bare integer (bytes).
pub fn parse_size(s: &str) -> Result<u64, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty size".into());
    }
    if let Ok(n) = s.parse::<u64>() {
        return Ok(n);
    }
    let (num_part, unit_part) = split_num_unit(s);
    let num: f64 = num_part
        .parse()
        .map_err(|e| format!("invalid size number `{num_part}`: {e}"))?;
    let mult: u64 = match unit_part.trim().to_ascii_uppercase().as_str() {
        "" | "B" => 1,
        "K" | "KB" | "KIB" => 1024,
        "M" | "MB" | "MIB" => 1024 * 1024,
        "G" | "GB" | "GIB" => 1024 * 1024 * 1024,
        other => return Err(format!("unknown size unit `{other}`")),
    };
    Ok((num * mult as f64) as u64)
}

/// Parse `"30s"`, `"5m"`, `"1h"`, `"1d"`, `"1y"`, or a bare integer (seconds).
/// Bare unit strings like `"m"` (without a count) are interpreted as `"1m"` so
/// rate-limit specs like `"5/m"` parse cleanly.
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty duration".into());
    }
    if let Ok(n) = s.parse::<u64>() {
        return Ok(Duration::from_secs(n));
    }
    let (num_part, unit_part) = split_num_unit(s);
    let num: u64 = if num_part.is_empty() {
        1
    } else {
        num_part
            .parse()
            .map_err(|e| format!("invalid duration number `{num_part}`: {e}"))?
    };
    let secs: u64 = match unit_part.trim().to_ascii_lowercase().as_str() {
        "s" | "sec" | "secs" | "second" | "seconds" => num,
        "m" | "min" | "mins" | "minute" | "minutes" => num * 60,
        "h" | "hr" | "hrs" | "hour" | "hours" => num * 3600,
        "d" | "day" | "days" => num * 86400,
        "w" | "wk" | "wks" | "week" | "weeks" => num * 86400 * 7,
        "mo" | "month" | "months" => num * 86400 * 30,
        "y" | "yr" | "yrs" | "year" | "years" => num * 86400 * 365,
        other => return Err(format!("unknown duration unit `{other}`")),
    };
    Ok(Duration::from_secs(secs))
}

fn split_num_unit(s: &str) -> (&str, &str) {
    let split = s
        .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
        .unwrap_or(s.len());
    (s[..split].trim(), s[split..].trim())
}

/// Parse `"60/minute"` → (count, window).
pub fn parse_rate(s: &str) -> Result<(u32, Duration), String> {
    let (count, window) = s
        .split_once('/')
        .ok_or_else(|| format!("rate must be `<count>/<window>`: {s}"))?;
    let count: u32 = count
        .trim()
        .parse()
        .map_err(|e| format!("invalid count `{count}`: {e}"))?;
    let dur = parse_duration(window.trim())?;
    Ok((count, dur))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sizes() {
        assert_eq!(parse_size("10").unwrap(), 10);
        assert_eq!(parse_size("10KB").unwrap(), 10 * 1024);
        assert_eq!(parse_size("2MB").unwrap(), 2 * 1024 * 1024);
        assert_eq!(parse_size("1GB").unwrap(), 1024 * 1024 * 1024);
        assert_eq!(parse_size("1.5MB").unwrap(), (1.5 * 1024.0 * 1024.0) as u64);
        assert!(parse_size("bad").is_err());
    }

    #[test]
    fn parses_durations() {
        assert_eq!(parse_duration("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_duration("5m").unwrap(), Duration::from_secs(300));
        assert_eq!(parse_duration("1h").unwrap(), Duration::from_secs(3600));
        assert_eq!(parse_duration("1d").unwrap(), Duration::from_secs(86400));
        assert_eq!(
            parse_duration("1y").unwrap(),
            Duration::from_secs(86400 * 365)
        );
        assert_eq!(parse_duration("42").unwrap(), Duration::from_secs(42));
        assert!(parse_duration("bad").is_err());
    }

    #[test]
    fn parses_rates() {
        let (count, win) = parse_rate("60/minute").unwrap();
        assert_eq!(count, 60);
        assert_eq!(win, Duration::from_secs(60));
        let (count, win) = parse_rate("5/m").unwrap();
        assert_eq!(count, 5);
        assert_eq!(win, Duration::from_secs(60));
    }

    #[test]
    fn loads_vhost_and_security_toml() {
        let toml = r#"
            bind = "0.0.0.0:443"
            server_name = ["example.com", "www.example.com", "*.example.com"]

            [tls]
            cert = "/etc/cert.pem"
            key  = "/etc/key.pem"

            [redirect_http]
            bind = "0.0.0.0:80"
            permanent = true
            target_host = "example.com"

            [hsts]
            enabled = true
            max_age = "1y"
            include_subdomains = true
            preload = false

            [cors]
            enabled = true
            allow_origins = ["*"]
            allow_credentials = false
            max_age = "1h"

            [[ip_rules]]
            prefix = "/admin"
            action = "allow"
            ranges = ["10.0.0.0/8"]

            [[basic_auth]]
            prefix = "/admin"
            realm = "Admin"
            credentials = ["alice:secret", "bob:second"]
        "#;
        let cfg: ServerConfig = toml::from_str(toml).unwrap();
        assert_eq!(
            cfg.server_name,
            vec!["example.com", "www.example.com", "*.example.com"]
        );
        assert!(cfg.redirect_http.is_some());
        assert_eq!(
            cfg.redirect_http.as_ref().unwrap().target_host.as_deref(),
            Some("example.com")
        );
        assert!(cfg.hsts.enabled);
        assert_eq!(cfg.hsts.max_age, Some(Duration::from_secs(86400 * 365)));
        assert!(cfg.cors.enabled);
        assert_eq!(cfg.ip_rules.len(), 1);
        assert_eq!(cfg.basic_auth.len(), 1);
        assert_eq!(cfg.basic_auth[0].credentials.len(), 2);
    }

    #[test]
    fn loads_rewrites_and_proxies_toml() {
        let toml = r#"
            [[rewrites]]
            from = "^/old/(.*)$"
            to = "/new/$1"
            status = 301

            [[rewrites]]
            from = "^/legacy/(.*)$"
            to = "/v2/$1"

            [trailing_slash]
            mode = "always"
            action = "redirect"

            [error_pages]
            404 = "errors/404.html"
            500 = "errors/500.html"

            [[proxy]]
            prefix = "/api/v2"
            upstream = "http://api-v2.internal:8080"
            strip_prefix = true
            timeout = "10s"
            retries = 3
        "#;
        let cfg: ServerConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.rewrites.len(), 2);
        assert_eq!(cfg.rewrites[0].status, Some(301));
        assert!(cfg.rewrites[1].status.is_none());
        assert_eq!(cfg.trailing_slash.mode, TrailingSlashMode::Always);
        assert_eq!(cfg.trailing_slash.action, TrailingSlashAction::Redirect);
        assert_eq!(cfg.error_pages.len(), 2);
        assert!(cfg.error_pages.contains_key("404"));
        assert_eq!(cfg.proxies.len(), 1);
        assert_eq!(cfg.proxies[0].upstream, "http://api-v2.internal:8080");
        assert_eq!(cfg.proxies[0].retries, 3);
        assert_eq!(cfg.proxies[0].timeout, Some(Duration::from_secs(10)));
    }

    #[test]
    fn loads_full_toml() {
        let toml = r#"
            bind = "0.0.0.0:443"

            [tls]
            cert = "/etc/letsencrypt/live/example.com/fullchain.pem"
            key = "/etc/letsencrypt/live/example.com/privkey.pem"

            [limits]
            body_max = "10MB"
            request_timeout = "30s"

            [compression]
            enabled = true
            algorithms = ["gzip", "br"]
            min_size = "1KB"

            [static_files."/assets"]
            dir = "public/build"
            cache = "1y"

            [rate_limit]
            per_ip = "60/minute"

            [rate_limit.routes]
            "POST /login" = "5/minute"

            [trusted_proxies]
            ranges = ["10.0.0.0/8", "127.0.0.1/32"]

            [access_log]
            format = "json"
            path = "storage/logs/access.log"
        "#;
        let cfg: ServerConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.bind, "0.0.0.0:443");
        assert!(cfg.tls.is_some());
        assert_eq!(cfg.limits.body_max, 10 * 1024 * 1024);
        assert_eq!(cfg.limits.request_timeout, Some(Duration::from_secs(30)));
        assert!(cfg.compression.enabled);
        assert_eq!(cfg.compression.algorithms, vec!["gzip", "br"]);
        assert_eq!(cfg.compression.min_size, 1024);
        assert!(cfg.static_files.contains_key("/assets"));
        assert_eq!(
            cfg.static_files["/assets"].cache,
            Some(Duration::from_secs(86400 * 365))
        );
        assert_eq!(cfg.rate_limit.per_ip.as_deref(), Some("60/minute"));
        assert_eq!(
            cfg.rate_limit.routes.get("POST /login").map(String::as_str),
            Some("5/minute")
        );
        assert_eq!(cfg.trusted_proxies.ranges.len(), 2);
        assert_eq!(cfg.access_log.format, AccessLogFormat::Json);
    }
}
