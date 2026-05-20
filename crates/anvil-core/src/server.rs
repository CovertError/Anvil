//! Production HTTP serving: applies the `ServerConfig` to an `axum::Router` and
//! starts it on the configured bind addr, with optional TLS via `axum-server`.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, Request, Response, StatusCode};
use axum::middleware::Next;
use axum::Router as AxumRouter;
use tower_http::compression::predicate::SizeAbove;
use tower_http::compression::CompressionLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

use crate::container::Container;
use crate::server_config::{
    AccessLogFormat, BasicAuthRule, CorsConfig, HstsConfig, IpAction, IpRule, ProxyRule,
    RateLimitConfig, RewriteRule, RouteTimeoutRule, ServerConfig, StaticMount, TlsConfig,
    TrailingSlashAction, TrailingSlashConfig, TrailingSlashMode,
};
use crate::Error;
use axum::extract::ConnectInfo;

/// Apply every layer the server config calls for to the user's web router,
/// then merge any static-file mounts. Returns a ready-to-serve `axum::Router`.
pub fn apply_layers(web: AxumRouter<Container>, cfg: &ServerConfig) -> AxumRouter<Container> {
    let mut router = web;

    // Static file mounts run BEFORE wrapping with body/timeout/compression — they
    // serve from disk and don't need request body parsing.
    for (prefix, mount) in &cfg.static_files {
        router = mount_static(router, prefix, mount);
    }

    // Compose request-side layers.
    let body_max = cfg.limits.body_max as usize;
    router = router
        .layer(RequestBodyLimitLayer::new(body_max))
        .layer(TraceLayer::new_for_http());

    if let Some(timeout) = cfg.limits.request_timeout {
        router = router.layer(TimeoutLayer::new(timeout));
    }

    // L4-ish concurrency cap. Tower's `GlobalConcurrencyLimitLayer` holds a
    // semaphore shared across every request; clones of the inner service
    // share the permit pool. Above the cap, new requests are answered with
    // 503 Service Unavailable (mapped from the `Overloaded` error) instead
    // of queueing indefinitely — protects against thundering-herd overload
    // and lets an upstream LB steer traffic to healthy peers.
    if let Some(max) = cfg.limits.max_concurrency {
        let max = max.max(1) as usize;
        let limiter = Arc::new(tokio::sync::Semaphore::new(max));
        let limiter_clone = limiter.clone();
        router = router.layer(axum::middleware::from_fn(
            move |req: Request<Body>, next: Next| {
                let limiter = limiter_clone.clone();
                async move {
                    match limiter.try_acquire() {
                        Ok(_permit) => next.run(req).await,
                        Err(_) => {
                            let mut resp = Response::new(Body::from(
                                "service overloaded — too many concurrent requests",
                            ));
                            *resp.status_mut() = StatusCode::SERVICE_UNAVAILABLE;
                            resp
                        }
                    }
                }
            },
        ));
    }

    // Per-route timeout overrides. Walk the request path against each
    // configured prefix; first match wins. Applied BEFORE the global
    // timeout layer above so that a slow upload endpoint can extend the
    // window rather than fighting it.
    if !cfg.route_timeouts.is_empty() {
        let rules: Arc<Vec<RouteTimeoutRule>> = Arc::new(cfg.route_timeouts.clone());
        let rules_clone = rules.clone();
        router = router.layer(axum::middleware::from_fn(
            move |req: Request<Body>, next: Next| {
                let rules = rules_clone.clone();
                async move { route_timeout_mw(rules, req, next).await }
            },
        ));
    }

    // Virtual-host gating: only accept requests whose Host header matches a
    // configured `server_name`. Empty `server_name` = match-all.
    if !cfg.server_name.is_empty() {
        let allowed = cfg.server_name.clone();
        router = router.layer(axum::middleware::from_fn(
            move |req: Request<Body>, next: Next| {
                let allowed = allowed.clone();
                async move { host_match_mw(allowed, req, next).await }
            },
        ));
    }

    // Trusted-proxy CIDR list — captured once per `apply_layers` call so each
    // middleware that reads the client IP can decide whether to honor
    // X-Forwarded-For. Empty = ignore XFF entirely (safe default for direct-
    // listen deployments).
    let trusted: Arc<Vec<ipnet::IpNet>> = Arc::new(cfg.trusted_proxies.ranges.clone());

    // IP allow/deny + basic auth — apply first so unauthorized requests don't
    // touch any other layer.
    if !cfg.ip_rules.is_empty() {
        let rules = Arc::new(cfg.ip_rules.clone());
        let rules_clone = rules.clone();
        let trusted_clone = trusted.clone();
        router = router.layer(axum::middleware::from_fn(
            move |req: Request<Body>, next: Next| {
                let rules = rules_clone.clone();
                let trusted = trusted_clone.clone();
                async move { ip_rules_mw(rules, trusted, req, next).await }
            },
        ));
    }
    if !cfg.basic_auth.is_empty() {
        let rules = Arc::new(compile_basic_auth(&cfg.basic_auth));
        let rules_clone = rules.clone();
        router = router.layer(axum::middleware::from_fn(
            move |req: Request<Body>, next: Next| {
                let rules = rules_clone.clone();
                async move { basic_auth_mw(rules, req, next).await }
            },
        ));
    }

    // CORS — apply early. tower-http's CorsLayer would be cleaner, but we want
    // full TOML control without depending on tower-http's CORS feature spec.
    if cfg.cors.enabled {
        let cors = Arc::new(cfg.cors.clone());
        let cors_clone = cors.clone();
        router = router.layer(axum::middleware::from_fn(
            move |req: Request<Body>, next: Next| {
                let cors = cors_clone.clone();
                async move { cors_mw(cors, req, next).await }
            },
        ));
    }

    // Reverse-proxy rules — apply BEFORE rewrites so the user can rewrite
    // upstream-bound requests too.
    if !cfg.proxies.is_empty() {
        let proxies = Arc::new(CompiledProxies::compile(&cfg.proxies));
        let proxies_clone = proxies.clone();
        router = router.layer(axum::middleware::from_fn(
            move |req: Request<Body>, next: Next| {
                let proxies = proxies_clone.clone();
                async move { proxy_mw(proxies, req, next).await }
            },
        ));
    }

    // Rewrites — apply early so they see the request before other layers.
    if !cfg.rewrites.is_empty() {
        let compiled = Arc::new(CompiledRewrites::compile(&cfg.rewrites));
        let compiled_clone = compiled.clone();
        router = router.layer(axum::middleware::from_fn(
            move |req: Request<Body>, next: Next| {
                let rules = compiled_clone.clone();
                async move { rewrite_mw(rules, req, next).await }
            },
        ));
    }

    // Trailing-slash policy.
    if cfg.trailing_slash.mode != TrailingSlashMode::Ignore {
        let ts = cfg.trailing_slash.clone();
        router = router.layer(axum::middleware::from_fn(
            move |req: Request<Body>, next: Next| {
                let ts = ts.clone();
                async move { trailing_slash_mw(ts, req, next).await }
            },
        ));
    }

    // Custom error pages: intercept responses with matching status codes and
    // substitute the configured file contents.
    if !cfg.error_pages.is_empty() {
        let pages = Arc::new(load_error_pages(&cfg.error_pages));
        let pages_clone = pages.clone();
        router = router.layer(axum::middleware::from_fn(
            move |req: Request<Body>, next: Next| {
                let pages = pages_clone.clone();
                async move { error_pages_mw(pages, req, next).await }
            },
        ));
    }

    // HSTS header for HTTPS responses.
    if cfg.tls.is_some() && cfg.hsts.enabled {
        if let Some(value) = build_hsts_header(&cfg.hsts) {
            router = router.layer(SetResponseHeaderLayer::if_not_present(
                HeaderName::from_static("strict-transport-security"),
                value,
            ));
        }
    }

    if cfg.compression.enabled {
        // tower-http's `CompressionLayer` selects the encoding based on the
        // client's `Accept-Encoding` header; we just toggle the layer on and
        // gate via the min-size predicate. Per-algorithm disable lives on the
        // un-parameterized layer, so we apply it before `compress_when`.
        let min_size = u16::try_from(cfg.compression.min_size).unwrap_or(u16::MAX);
        let mut layer = CompressionLayer::new();
        if !cfg
            .compression
            .algorithms
            .iter()
            .any(|a| a.eq_ignore_ascii_case("gzip"))
        {
            layer = layer.no_gzip();
        }
        if !cfg
            .compression
            .algorithms
            .iter()
            .any(|a| a.eq_ignore_ascii_case("br") || a.eq_ignore_ascii_case("brotli"))
        {
            layer = layer.no_br();
        }
        if !cfg
            .compression
            .algorithms
            .iter()
            .any(|a| a.eq_ignore_ascii_case("deflate"))
        {
            layer = layer.no_deflate();
        }
        let layer = layer.compress_when(SizeAbove::new(min_size));
        router = router.layer(layer);
    }

    if cfg.rate_limit.per_ip.is_some() || !cfg.rate_limit.routes.is_empty() {
        let limiter = Arc::new(RateLimiter::from_config(&cfg.rate_limit));
        let limiter_clone = limiter.clone();
        let trusted_clone = trusted.clone();
        router = router.layer(axum::middleware::from_fn(
            move |req: Request<Body>, next: Next| {
                let limiter = limiter_clone.clone();
                let trusted = trusted_clone.clone();
                async move { rate_limit_mw(limiter, trusted, req, next).await }
            },
        ));
    }

    if matches!(
        cfg.access_log.format,
        AccessLogFormat::Combined | AccessLogFormat::Json
    ) {
        let format = cfg.access_log.format;
        let trusted_clone = trusted.clone();
        router = router.layer(axum::middleware::from_fn(
            move |req: Request<Body>, next: Next| {
                let trusted = trusted_clone.clone();
                async move { access_log_mw(format, trusted, req, next).await }
            },
        ));
    }

    // Request ID — generated if the inbound request didn't carry one, echoed
    // back on the response. Threads through `tracing` so each log line for
    // a given request shares a `request_id` field, which is the basic
    // building block of trace correlation across services.
    router = router.layer(axum::middleware::from_fn(request_id_mw));

    router
}

fn mount_static(
    router: AxumRouter<Container>,
    prefix: &str,
    mount: &StaticMount,
) -> AxumRouter<Container> {
    // Note: `ranges` is reserved for a future version of tower-http that exposes
    // per-instance range toggling. For now ranges are always enabled.
    let _ = mount.ranges;

    // If the app registered an embedded-asset fetcher for this prefix (the
    // single-binary distribution path), serve from memory instead of disk.
    if let Some(fetcher) = crate::embedded::lookup(prefix) {
        let cache = mount.cache;
        let route_pat = format!("{}/*path", prefix.trim_end_matches('/'));
        let nested = AxumRouter::<Container>::new().route(
            &route_pat,
            axum::routing::get(
                move |axum::extract::Path(path): axum::extract::Path<String>,
                      headers: HeaderMap| async move {
                    serve_embedded(fetcher, cache, &path, &headers)
                },
            ),
        );
        return router.merge(nested);
    }

    let svc = ServeDir::new(&mount.dir);

    let nested = AxumRouter::<Container>::new().nest_service(prefix, svc);
    let nested = if let Some(cache) = mount.cache {
        let value = HeaderValue::from_str(&format!("public, max-age={}", cache.as_secs()))
            .unwrap_or_else(|_| HeaderValue::from_static("public"));
        nested.layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("cache-control"),
            value,
        ))
    } else {
        nested
    };
    router.merge(nested)
}

/// Embedded-asset request handler: looks up the wildcard `path` in the
/// registered fetcher, honors `If-None-Match` against the file's ETag, and
/// returns 200/304/404.
fn serve_embedded(
    fetcher: crate::embedded::EmbeddedAssetFetcher,
    cache: Option<Duration>,
    path: &str,
    headers: &HeaderMap,
) -> Response<Body> {
    let asset = match fetcher(path) {
        Some(a) => a,
        None => return not_found(),
    };

    if let (Some(client_tag), Some(asset_tag)) = (
        headers
            .get(axum::http::header::IF_NONE_MATCH)
            .and_then(|v| v.to_str().ok()),
        asset.etag.as_deref(),
    ) {
        if etag_matches(client_tag, asset_tag) {
            let mut resp = Response::builder()
                .status(StatusCode::NOT_MODIFIED)
                .body(Body::empty())
                .expect("304 body");
            if let Some(d) = cache {
                if let Ok(v) = HeaderValue::from_str(&format!("public, max-age={}", d.as_secs()))
                {
                    resp.headers_mut().insert("cache-control", v);
                }
            }
            return resp;
        }
    }

    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header("content-type", asset.content_type.as_str())
        .header("content-length", asset.data.len());
    if let Some(tag) = asset.etag.as_deref() {
        builder = builder.header("etag", quote_etag(tag));
    }
    if let Some(d) = cache {
        builder = builder.header("cache-control", format!("public, max-age={}", d.as_secs()));
    }
    builder
        .body(Body::from(asset.data.into_owned()))
        .unwrap_or_else(|_| not_found())
}

fn not_found() -> Response<Body> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Body::from("not found"))
        .expect("404 body")
}

fn quote_etag(raw: &str) -> String {
    if raw.starts_with('"') {
        raw.to_string()
    } else {
        format!("\"{raw}\"")
    }
}

fn etag_matches(client: &str, server: &str) -> bool {
    let normalize = |s: &str| -> String {
        s.split(',')
            .map(|t| t.trim().trim_matches('"').trim_start_matches("W/").to_string())
            .collect::<Vec<_>>()
            .join(",")
    };
    let server_norm = normalize(server);
    normalize(client)
        .split(',')
        .any(|tag| tag == server_norm || tag == "*")
}

// ─── Serve entry points ─────────────────────────────────────────────────────

pub async fn serve(
    router: AxumRouter,
    cfg: &ServerConfig,
    shutdown: tokio::sync::oneshot::Receiver<()>,
) -> Result<(), Error> {
    let addr: SocketAddr = cfg
        .bind
        .parse()
        .map_err(|e| Error::Config(format!("invalid bind addr `{}`: {e}", cfg.bind)))?;

    tracing::info!(%addr, tls = cfg.tls.is_some(), server_name = ?cfg.server_name, "anvil server starting");

    // If a redirect-HTTP listener is configured, spawn it alongside the main listener.
    let (shutdown_main_tx, shutdown_main_rx) = tokio::sync::oneshot::channel::<()>();
    let (shutdown_redir_tx, shutdown_redir_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let _ = shutdown.await;
        let _ = shutdown_main_tx.send(());
        let _ = shutdown_redir_tx.send(());
    });

    let redirect_task = cfg.redirect_http.clone().map(|redir| {
        let target_host = redir
            .target_host
            .clone()
            .or_else(|| cfg.server_name.first().cloned());
        let permanent = redir.permanent;
        let bind = redir.bind.clone();
        tokio::spawn(async move {
            if let Err(e) =
                serve_redirect_http(&bind, target_host, permanent, shutdown_redir_rx).await
            {
                tracing::warn!(?e, "redirect_http listener exited with error");
            }
        })
    });

    let main_result = if let Some(tls) = &cfg.tls {
        if tls.acme.is_some() {
            serve_acme(router, addr, tls, cfg.limits.drain_timeout, shutdown_main_rx).await
        } else {
            serve_tls(router, addr, tls, cfg.limits.drain_timeout, shutdown_main_rx).await
        }
    } else {
        serve_plain(router, addr, shutdown_main_rx).await
    };

    if let Some(task) = redirect_task {
        task.abort();
    }

    main_result
}

/// Plain-HTTP listener that 30x-redirects every request to its `https://`
/// equivalent. Used when TLS is on and `redirect_http` is configured.
async fn serve_redirect_http(
    bind: &str,
    target_host: Option<String>,
    permanent: bool,
    shutdown: tokio::sync::oneshot::Receiver<()>,
) -> Result<(), Error> {
    let addr: SocketAddr = bind
        .parse()
        .map_err(|e| Error::Config(format!("invalid redirect_http bind `{bind}`: {e}")))?;
    tracing::info!(%addr, target_host = ?target_host, permanent, "http→https redirect listener");

    let target_host = Arc::new(target_host);
    let router: AxumRouter = AxumRouter::new().fallback(axum::routing::any({
        let target_host = target_host.clone();
        move |req: Request<Body>| {
            let target_host = target_host.clone();
            async move { http_redirect_handler(req, target_host, permanent).await }
        }
    }));

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            let _ = shutdown.await;
        })
        .await?;
    Ok(())
}

async fn http_redirect_handler(
    req: Request<Body>,
    target_host: Arc<Option<String>>,
    permanent: bool,
) -> Response<Body> {
    let host = target_host.as_ref().clone().unwrap_or_else(|| {
        req.headers()
            .get("host")
            .and_then(|v| v.to_str().ok())
            .map(String::from)
            .unwrap_or_default()
    });
    let path_and_query = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| "/".to_string());
    let location = format!("https://{host}{path_and_query}");

    let status = if permanent {
        StatusCode::MOVED_PERMANENTLY
    } else {
        StatusCode::FOUND
    };
    let mut resp = Response::new(Body::from(format!("Redirecting to {location}\n")));
    *resp.status_mut() = status;
    if let Ok(loc) = HeaderValue::from_str(&location) {
        resp.headers_mut().insert("location", loc);
    }
    resp
}

fn build_hsts_header(cfg: &HstsConfig) -> Option<HeaderValue> {
    let max_age = cfg.max_age.unwrap_or(Duration::from_secs(86400 * 365));
    let mut value = format!("max-age={}", max_age.as_secs());
    if cfg.include_subdomains {
        value.push_str("; includeSubDomains");
    }
    if cfg.preload {
        value.push_str("; preload");
    }
    HeaderValue::from_str(&value).ok()
}

/// Reject requests whose Host header doesn't match any configured server_name.
async fn host_match_mw(allowed: Vec<String>, req: Request<Body>, next: Next) -> Response<Body> {
    let host = req
        .headers()
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    // Strip port for matching: "example.com:8080" → "example.com".
    let host_no_port = host.split(':').next().unwrap_or("").to_ascii_lowercase();

    if matches_any(&host_no_port, &allowed) {
        return next.run(req).await;
    }

    tracing::debug!(host, allowed = ?allowed, "rejected host: no server_name match");
    let mut resp = Response::new(Body::from(format!(
        "404 not found (unknown host: {host})\n"
    )));
    *resp.status_mut() = StatusCode::NOT_FOUND;
    resp
}

fn matches_any(host: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|pat| matches_pattern(host, pat))
}

/// Match a host against a pattern. Supports exact match and `*.example.com`
/// wildcards. The pattern is normalized to lowercase. A bare `*` matches any.
fn matches_pattern(host: &str, pattern: &str) -> bool {
    let pattern = pattern.to_ascii_lowercase();
    if pattern == "*" {
        return true;
    }
    if let Some(suffix) = pattern.strip_prefix("*.") {
        // `*.foo.com` matches `bar.foo.com` but not `foo.com`.
        return host.ends_with(&format!(".{suffix}"));
    }
    host == pattern
}

async fn serve_plain(
    router: AxumRouter,
    addr: SocketAddr,
    shutdown: tokio::sync::oneshot::Receiver<()>,
) -> Result<(), Error> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        let _ = shutdown.await;
    })
    .await?;
    Ok(())
}

async fn serve_tls(
    router: AxumRouter,
    addr: SocketAddr,
    tls: &TlsConfig,
    drain: Duration,
    shutdown: tokio::sync::oneshot::Receiver<()>,
) -> Result<(), Error> {
    // Path A: single cert pair → fast happy path, no resolver indirection.
    // Path B: `[[tls.certs]]` entries present → build a custom
    // `ResolvesServerCert` that picks the cert by ClientHello SNI hostname,
    // with the top-level cert as the default for unmatched names.
    let config = if tls.additional_certs.is_empty() {
        axum_server::tls_rustls::RustlsConfig::from_pem_file(&tls.cert, &tls.key)
            .await
            .map_err(|e| Error::Config(format!("tls load: {e}")))?
    } else {
        let resolver = build_sni_resolver(tls)
            .map_err(|e| Error::Config(format!("tls multi-cert load: {e}")))?;
        let server_config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_cert_resolver(Arc::new(resolver));
        axum_server::tls_rustls::RustlsConfig::from_config(Arc::new(server_config))
    };

    // Cert hot-reload: spawn a notify watcher on the cert + key paths. On
    // any change, re-read the PEM files and `reload_from_pem_file` on the
    // shared RustlsConfig — new TLS handshakes pick up the new cert without
    // a process restart, dropping the "swap cert → restart server" runbook
    // ops normally need.
    let watch_paths = [tls.cert.clone(), tls.key.clone()];
    let config_for_watch = config.clone();
    let cert_path = tls.cert.clone();
    let key_path = tls.key.clone();
    tokio::task::spawn_blocking(move || {
        if let Err(e) = watch_tls_certs(config_for_watch, cert_path, key_path, watch_paths) {
            tracing::warn!(error = %e, "cert hot-reload watcher exited");
        }
    });

    let handle = axum_server::Handle::new();
    let handle_for_shutdown = handle.clone();
    tokio::spawn(async move {
        let _ = shutdown.await;
        handle_for_shutdown.graceful_shutdown(Some(drain));
    });

    axum_server::bind_rustls(addr, config)
        .handle(handle)
        .serve(router.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .map_err(|e| Error::Internal(format!("tls serve: {e}")))?;
    Ok(())
}

/// ACME-managed TLS serve path. Auto-obtains and rotates certs via Let's
/// Encrypt (or any other ACME directory) using TLS-ALPN-01 in-process — no
/// external certbot run required.
///
/// **Status:** the `[tls.acme]` schema parses today and apps written
/// against it are forward-compatible. The runtime implementation is held
/// back pending a focused PR that pins compatible `rustls-acme` /
/// `rustls` / `axum-server` versions; the upstream `rustls-acme` 0.13
/// release has build errors against the rustls version this workspace
/// pins for the rest of TLS. Until that PR lands, ACME configs surface
/// as a clear startup error rather than silently no-op'ing.
async fn serve_acme(
    _router: AxumRouter,
    _addr: SocketAddr,
    tls: &TlsConfig,
    _drain: Duration,
    _shutdown: tokio::sync::oneshot::Receiver<()>,
) -> Result<(), Error> {
    let acme = tls
        .acme
        .as_ref()
        .expect("serve_acme called without [tls.acme]");
    Err(Error::Config(format!(
        "[tls.acme] is configured for {n} domain(s) but ACME runtime support \
         is pending a follow-up PR (rustls-acme version pin). For now, use \
         certbot in TLS-ALPN-01 mode and `[tls] cert`/`key` pointing at the \
         certbot output; cert hot-reload handles renewals without restart.",
        n = acme.domains.len(),
    )))
}

/// SNI cert resolver — picks a `CertifiedKey` based on the ClientHello's
/// SNI hostname. Falls back to the default cert when no entry matches.
/// Matches `server_name` patterns the same way as the host-gating middleware:
/// exact match, `*.example.com` wildcard for one-level subdomains, or `*` for
/// any host.
#[derive(Debug)]
struct SniResolver {
    /// Pre-compiled `(server_name, CertifiedKey)` pairs in declaration order.
    /// First match wins, so put the most specific patterns first.
    entries: Vec<(String, Arc<rustls::sign::CertifiedKey>)>,
    default_key: Arc<rustls::sign::CertifiedKey>,
}

impl rustls::server::ResolvesServerCert for SniResolver {
    fn resolve(
        &self,
        client_hello: rustls::server::ClientHello<'_>,
    ) -> Option<Arc<rustls::sign::CertifiedKey>> {
        let sni = client_hello.server_name().unwrap_or("").to_ascii_lowercase();
        for (pattern, key) in &self.entries {
            if matches_pattern(&sni, pattern) {
                return Some(key.clone());
            }
        }
        Some(self.default_key.clone())
    }
}

fn build_sni_resolver(tls: &TlsConfig) -> std::io::Result<SniResolver> {
    let default_key = load_certified_key(&tls.cert, &tls.key)?;
    let mut entries = Vec::with_capacity(tls.additional_certs.len());
    for entry in &tls.additional_certs {
        let key = load_certified_key(&entry.cert, &entry.key)?;
        entries.push((entry.server_name.to_ascii_lowercase(), key));
    }
    tracing::info!(
        default_cert = %tls.cert.display(),
        additional = tls.additional_certs.len(),
        "tls: SNI resolver active"
    );
    Ok(SniResolver { entries, default_key })
}

fn load_certified_key(
    cert_path: &std::path::Path,
    key_path: &std::path::Path,
) -> std::io::Result<Arc<rustls::sign::CertifiedKey>> {
    use std::io::BufReader;

    let cert_file = std::fs::File::open(cert_path).map_err(|e| {
        std::io::Error::new(e.kind(), format!("opening cert {}: {e}", cert_path.display()))
    })?;
    let mut cert_reader = BufReader::new(cert_file);
    let certs: Vec<rustls::pki_types::CertificateDer<'static>> = rustls_pemfile::certs(&mut cert_reader)
        .collect::<std::io::Result<_>>()?;
    if certs.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("no certificates in {}", cert_path.display()),
        ));
    }

    let key_file = std::fs::File::open(key_path).map_err(|e| {
        std::io::Error::new(e.kind(), format!("opening key {}: {e}", key_path.display()))
    })?;
    let mut key_reader = BufReader::new(key_file);
    let key = rustls_pemfile::private_key(&mut key_reader)?.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("no private key in {}", key_path.display()),
        )
    })?;

    let signing_key = rustls::crypto::ring::sign::any_supported_type(&key)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("sign: {e}")))?;

    Ok(Arc::new(rustls::sign::CertifiedKey::new(certs, signing_key)))
}

/// Blocking-thread watcher that re-loads the rustls config whenever the cert
/// or key file on disk changes. Uses `notify` (already a workspace dep for
/// the dev file watcher) so we don't add anything new.
///
/// Coalescing: many editors write atomically by rename, which produces
/// `Modify`+`Create` events in quick succession; we ignore that and just
/// reload on any non-error event. The reload is itself cheap and idempotent.
fn watch_tls_certs(
    config: axum_server::tls_rustls::RustlsConfig,
    cert: std::path::PathBuf,
    key: std::path::PathBuf,
    watch_paths: [std::path::PathBuf; 2],
) -> std::io::Result<()> {
    use notify::{RecursiveMode, Watcher};
    use std::sync::mpsc::channel;

    let (tx, rx) = channel::<notify::Result<notify::Event>>();
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })
    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("notify init: {e}")))?;

    // Watch the parent directories — file-level watches don't survive
    // editors that rename-on-write (vim, cargo, etc.), but directory watches
    // catch the rename plus the new file's creation.
    for p in &watch_paths {
        if let Some(parent) = p.parent() {
            watcher
                .watch(parent, RecursiveMode::NonRecursive)
                .map_err(|e| {
                    std::io::Error::new(std::io::ErrorKind::Other, format!("notify watch: {e}"))
                })?;
        }
    }

    let runtime = tokio::runtime::Handle::try_current().ok();
    loop {
        let Ok(event) = rx.recv() else { break };
        let Ok(event) = event else { continue };
        // Only react to events touching our cert/key files specifically — the
        // directory watcher fires for any sibling file too.
        let touches_us = event.paths.iter().any(|p| p == &cert || p == &key);
        if !touches_us {
            continue;
        }
        tracing::info!(
            cert = %cert.display(),
            key = %key.display(),
            "tls cert change detected — reloading"
        );
        let cert = cert.clone();
        let key = key.clone();
        let config = config.clone();
        let reload = async move {
            if let Err(e) = config.reload_from_pem_file(&cert, &key).await {
                tracing::warn!(error = %e, "tls reload failed");
            } else {
                tracing::info!("tls cert reloaded successfully");
            }
        };
        if let Some(rt) = &runtime {
            rt.spawn(reload);
        } else {
            // No current tokio runtime (e.g. unit test contexts) — best-effort
            // fire-and-forget via a fresh single-threaded runtime.
            std::thread::spawn(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                if let Ok(rt) = rt {
                    rt.block_on(reload);
                }
            });
        }
    }
    Ok(())
}

// ─── Rate limiter (Moka-backed token bucket) ────────────────────────────────

pub struct RateLimiter {
    /// `bucket key` → `(window_end_instant, count_remaining)`.
    state: moka::sync::Cache<String, (Instant, u32)>,
    default_rule: Option<RateRule>,
    route_rules: Vec<(MatchKey, RateRule)>,
}

#[derive(Clone, Copy)]
struct RateRule {
    count: u32,
    window: Duration,
}

#[derive(Clone)]
struct MatchKey {
    method: Option<Method>,
    path: String,
}

impl RateLimiter {
    pub fn from_config(cfg: &RateLimitConfig) -> Self {
        let default_rule = cfg.per_ip.as_deref().and_then(|s| {
            crate::server_config::parse_rate(s)
                .map(|(count, window)| RateRule { count, window })
                .ok()
        });
        let route_rules = cfg
            .routes
            .iter()
            .filter_map(|(spec, rate)| {
                let (count, window) = crate::server_config::parse_rate(rate).ok()?;
                let (method, path) = parse_route_spec(spec);
                Some((MatchKey { method, path }, RateRule { count, window }))
            })
            .collect();

        Self {
            state: moka::sync::Cache::builder()
                .max_capacity(10_000)
                .time_to_idle(Duration::from_secs(600))
                .build(),
            default_rule,
            route_rules,
        }
    }

    fn rule_for(&self, method: &Method, path: &str) -> Option<RateRule> {
        for (key, rule) in &self.route_rules {
            if key.path == path && key.method.as_ref().is_none_or(|m| m == method) {
                return Some(*rule);
            }
        }
        self.default_rule
    }

    fn check(&self, bucket: &str, rule: RateRule) -> bool {
        let now = Instant::now();
        let mut allowed = true;
        self.state
            .entry(bucket.to_string())
            .and_compute_with(|existing| match existing {
                Some(entry) => {
                    let (window_end, count) = entry.into_value();
                    if now >= window_end {
                        moka::ops::compute::Op::Put((
                            now + rule.window,
                            rule.count.saturating_sub(1),
                        ))
                    } else if count > 0 {
                        moka::ops::compute::Op::Put((window_end, count - 1))
                    } else {
                        allowed = false;
                        moka::ops::compute::Op::Put((window_end, 0))
                    }
                }
                None => {
                    moka::ops::compute::Op::Put((now + rule.window, rule.count.saturating_sub(1)))
                }
            });
        allowed
    }
}

fn parse_route_spec(spec: &str) -> (Option<Method>, String) {
    let trimmed = spec.trim();
    if let Some((m, p)) = trimmed.split_once(char::is_whitespace) {
        let method = m.parse::<Method>().ok();
        (method, p.trim().to_string())
    } else {
        (None, trimmed.to_string())
    }
}

async fn rate_limit_mw(
    limiter: Arc<RateLimiter>,
    trusted: Arc<Vec<ipnet::IpNet>>,
    req: Request<Body>,
    next: Next,
) -> Response<Body> {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let bucket = format!("{}|{}|{}", client_ip(&req, &trusted), method, path);

    if let Some(rule) = limiter.rule_for(&method, &path) {
        if !limiter.check(&bucket, rule) {
            tracing::debug!(%method, %path, %bucket, "rate limited");
            let mut resp = Response::new(Body::from("rate limit exceeded"));
            *resp.status_mut() = StatusCode::TOO_MANY_REQUESTS;
            return resp;
        }
    }
    next.run(req).await
}

/// Resolve the client's IP, honoring `X-Forwarded-For` **only** when the
/// direct TCP peer is in `trusted` (set via `[trusted_proxies] ranges` in the
/// server config).
///
/// Order of precedence:
///   1. TCP peer (`ConnectInfo<SocketAddr>`) is in a trusted CIDR ⇒ take the
///      first hop in XFF (the original client per RFC 7239).
///   2. Otherwise ⇒ the TCP peer is the client. XFF from an untrusted peer
///      is ignored, so rate limits and IP rules can't be spoofed by hostile
///      clients setting their own XFF header.
///   3. No `ConnectInfo` extension (rare — only if the router was started
///      without `into_make_service_with_connect_info`) ⇒ `"unknown"`.
fn client_ip(req: &Request<Body>, trusted: &[ipnet::IpNet]) -> String {
    let peer: Option<std::net::IpAddr> = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip());

    let peer_trusted = match peer {
        Some(addr) => !trusted.is_empty() && trusted.iter().any(|net| net.contains(&addr)),
        None => false,
    };

    if peer_trusted {
        if let Some(v) = req.headers().get("x-forwarded-for") {
            if let Ok(s) = v.to_str() {
                if let Some(first) = s.split(',').next() {
                    let candidate = first.trim();
                    if !candidate.is_empty() {
                        return candidate.to_string();
                    }
                }
            }
        }
    }

    peer.map(|a| a.to_string()).unwrap_or_else(|| "unknown".into())
}

// ─── Access log ─────────────────────────────────────────────────────────────

/// Apply a per-route timeout to a request whose path matches one of the
/// configured prefixes. First match wins. Requests that don't match any
/// rule fall through to the global `limits.request_timeout` (if any).
async fn route_timeout_mw(
    rules: Arc<Vec<RouteTimeoutRule>>,
    req: Request<Body>,
    next: Next,
) -> Response<Body> {
    let path = req.uri().path().to_string();
    let matching = rules.iter().find(|r| path.starts_with(&r.prefix));
    match matching {
        Some(rule) => {
            let timeout = rule.timeout;
            match tokio::time::timeout(timeout, next.run(req)).await {
                Ok(resp) => resp,
                Err(_) => {
                    tracing::debug!(
                        path,
                        timeout_ms = timeout.as_millis(),
                        "route timeout exceeded"
                    );
                    let mut resp = Response::new(Body::from("request timed out"));
                    *resp.status_mut() = StatusCode::REQUEST_TIMEOUT;
                    resp
                }
            }
        }
        None => next.run(req).await,
    }
}

/// Generate or pass through `x-request-id`. If the inbound request carries
/// one, reuse it (lets upstream LBs / clients drive the trace ID); otherwise
/// mint a UUID v7 (time-ordered, sortable). Always echoed on the response so
/// log entries can be correlated by the same id from either side.
async fn request_id_mw(mut req: Request<Body>, next: Next) -> Response<Body> {
    const HEADER: &str = "x-request-id";

    let inbound = req
        .headers()
        .get(HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let request_id = inbound.unwrap_or_else(|| uuid::Uuid::now_v7().to_string());

    if let Ok(v) = HeaderValue::from_str(&request_id) {
        req.headers_mut().insert(HeaderName::from_static(HEADER), v.clone());
        // Run the handler with the id available in the request extensions so
        // downstream code (and the trace layer) can read it without parsing
        // headers again.
        req.extensions_mut().insert(RequestId(request_id.clone()));

        let mut resp = next.run(req).await;
        resp.headers_mut().insert(HeaderName::from_static(HEADER), v);
        resp
    } else {
        next.run(req).await
    }
}

/// Typed wrapper for the per-request id, stored in `Request::extensions`
/// by the request-id middleware. Handlers can pull it via
/// `req.extensions().get::<RequestId>()` (or via the axum
/// `Extension<RequestId>` extractor).
#[derive(Debug, Clone)]
pub struct RequestId(pub String);

async fn access_log_mw(
    format: AccessLogFormat,
    trusted: Arc<Vec<ipnet::IpNet>>,
    req: Request<Body>,
    next: Next,
) -> Response<Body> {
    let started = Instant::now();
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let host = req
        .headers()
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-")
        .to_string();
    let referer = req
        .headers()
        .get("referer")
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    let ua = req
        .headers()
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    let ip = client_ip(&req, &trusted);
    let request_id = req
        .extensions()
        .get::<RequestId>()
        .map(|id| id.0.clone());

    let resp = next.run(req).await;
    let elapsed = started.elapsed();
    let status = resp.status().as_u16();
    let bytes = response_size(resp.headers()).unwrap_or(0);

    match format {
        AccessLogFormat::Combined => {
            tracing::info!(
                target: "access_log",
                "{} - - \"{} {} HTTP/1.1\" {} {} \"{}\" \"{}\" {}ms",
                ip,
                method,
                path,
                status,
                bytes,
                referer.as_deref().unwrap_or("-"),
                ua.as_deref().unwrap_or("-"),
                elapsed.as_millis(),
            );
        }
        AccessLogFormat::Json => {
            tracing::info!(
                target: "access_log",
                json = %serde_json::json!({
                    "ip": ip,
                    "method": method.as_str(),
                    "path": path,
                    "host": host,
                    "status": status,
                    "bytes": bytes,
                    "referer": referer,
                    "user_agent": ua,
                    "duration_ms": elapsed.as_millis(),
                    "request_id": request_id,
                }),
                "request"
            );
        }
        AccessLogFormat::Off => {}
    }
    resp
}

fn response_size(headers: &HeaderMap) -> Option<u64> {
    headers
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
}

// ─── Rewrites ───────────────────────────────────────────────────────────────

#[derive(Clone)]
struct CompiledRewrite {
    pattern: regex::Regex,
    to: String,
    status: Option<u16>,
    match_query: bool,
}

struct CompiledRewrites {
    rules: Vec<CompiledRewrite>,
}

impl CompiledRewrites {
    fn compile(rules: &[RewriteRule]) -> Self {
        let compiled = rules
            .iter()
            .filter_map(|r| match regex::Regex::new(&r.from) {
                Ok(pattern) => Some(CompiledRewrite {
                    pattern,
                    to: r.to.clone(),
                    status: r.status,
                    match_query: r.match_query,
                }),
                Err(e) => {
                    tracing::warn!(rule = %r.from, error = %e, "invalid rewrite regex, skipping");
                    None
                }
            })
            .collect();
        Self { rules: compiled }
    }
}

async fn rewrite_mw(
    rules: Arc<CompiledRewrites>,
    mut req: Request<Body>,
    next: Next,
) -> Response<Body> {
    let path = req.uri().path().to_string();
    let path_and_query = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| path.clone());

    let target_str_path = path.clone();
    let target_str_full = path_and_query.clone();

    // First pass: apply path-only rules.
    let mut applied: Option<(String, Option<u16>)> = None;
    for rule in &rules.rules {
        let subject = if rule.match_query {
            &target_str_full
        } else {
            &target_str_path
        };
        if rule.pattern.is_match(subject) {
            let replaced = rule.pattern.replace(subject, rule.to.as_str()).to_string();
            applied = Some((replaced, rule.status));
            break;
        }
    }

    let Some((new_target, status)) = applied else {
        return next.run(req).await;
    };

    match status {
        Some(code @ (301 | 302 | 303 | 307 | 308)) => {
            let mut resp = Response::new(Body::from(format!("Redirecting to {new_target}\n")));
            *resp.status_mut() =
                StatusCode::from_u16(code).unwrap_or(StatusCode::MOVED_PERMANENTLY);
            if let Ok(loc) = HeaderValue::from_str(&new_target) {
                resp.headers_mut().insert("location", loc);
            }
            resp
        }
        _ => {
            // Internal rewrite: replace the URI's path-and-query.
            let mut parts = req.uri().clone().into_parts();
            if let Ok(new_pq) = new_target.parse::<axum::http::uri::PathAndQuery>() {
                parts.path_and_query = Some(new_pq);
            }
            if let Ok(new_uri) = axum::http::Uri::from_parts(parts) {
                *req.uri_mut() = new_uri;
            }
            next.run(req).await
        }
    }
}

// ─── Trailing slash ─────────────────────────────────────────────────────────

async fn trailing_slash_mw(
    cfg: TrailingSlashConfig,
    mut req: Request<Body>,
    next: Next,
) -> Response<Body> {
    let path = req.uri().path().to_string();
    if path == "/" {
        return next.run(req).await;
    }

    let want_slash = matches!(cfg.mode, TrailingSlashMode::Always);
    let has_slash = path.ends_with('/');

    if want_slash == has_slash {
        return next.run(req).await;
    }

    let new_path = if want_slash {
        format!("{path}/")
    } else {
        path.trim_end_matches('/').to_string()
    };

    let query = req
        .uri()
        .query()
        .map(|q| format!("?{q}"))
        .unwrap_or_default();
    let new_target = format!("{new_path}{query}");

    match cfg.action {
        TrailingSlashAction::Redirect => {
            let mut resp = Response::new(Body::from(format!("Redirecting to {new_target}\n")));
            *resp.status_mut() = StatusCode::MOVED_PERMANENTLY;
            if let Ok(loc) = HeaderValue::from_str(&new_target) {
                resp.headers_mut().insert("location", loc);
            }
            resp
        }
        TrailingSlashAction::Rewrite => {
            let mut parts = req.uri().clone().into_parts();
            if let Ok(pq) = new_target.parse::<axum::http::uri::PathAndQuery>() {
                parts.path_and_query = Some(pq);
            }
            if let Ok(new_uri) = axum::http::Uri::from_parts(parts) {
                *req.uri_mut() = new_uri;
            }
            next.run(req).await
        }
    }
}

// ─── Custom error pages ─────────────────────────────────────────────────────

struct LoadedErrorPages {
    by_status: std::collections::HashMap<u16, (String, &'static str)>,
}

fn load_error_pages(
    raw: &std::collections::BTreeMap<String, std::path::PathBuf>,
) -> LoadedErrorPages {
    let mut by_status = std::collections::HashMap::new();
    for (key, path) in raw {
        let Ok(code) = key.parse::<u16>() else {
            tracing::warn!(key, "error_pages: invalid status code, skipping");
            continue;
        };
        let body = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(?path, ?e, "error_pages: failed to read file, skipping");
                continue;
            }
        };
        let content_type = guess_content_type(path);
        by_status.insert(code, (body, content_type));
    }
    LoadedErrorPages { by_status }
}

fn guess_content_type(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") | Some("htm") => "text/html; charset=utf-8",
        Some("json") => "application/json",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "text/plain; charset=utf-8",
    }
}

async fn error_pages_mw(
    pages: Arc<LoadedErrorPages>,
    req: Request<Body>,
    next: Next,
) -> Response<Body> {
    let resp = next.run(req).await;
    let status = resp.status().as_u16();

    let Some((body, ctype)) = pages.by_status.get(&status) else {
        return resp;
    };

    let mut out = Response::new(Body::from(body.clone()));
    *out.status_mut() = resp.status();
    if let Ok(ct) = HeaderValue::from_str(ctype) {
        out.headers_mut().insert("content-type", ct);
    }
    // Preserve a couple of useful headers from the original response.
    for h in ["cache-control", "x-request-id"] {
        if let Some(v) = resp.headers().get(h) {
            out.headers_mut().insert(h, v.clone());
        }
    }
    out
}

// ─── Reverse proxy ──────────────────────────────────────────────────────────

#[derive(Clone)]
struct CompiledProxy {
    prefix: String,
    upstream: String,
    strip_prefix: bool,
    preserve_host: bool,
    timeout: Duration,
    retries: u8,
}

struct CompiledProxies {
    rules: Vec<CompiledProxy>,
    client: reqwest::Client,
}

impl CompiledProxies {
    fn compile(rules: &[ProxyRule]) -> Self {
        let mut compiled: Vec<CompiledProxy> = rules
            .iter()
            .map(|r| CompiledProxy {
                prefix: r.prefix.clone(),
                upstream: r.upstream.trim_end_matches('/').to_string(),
                strip_prefix: r.strip_prefix,
                preserve_host: r.preserve_host,
                timeout: r.timeout.unwrap_or(Duration::from_secs(30)),
                retries: r.retries,
            })
            .collect();
        // Longest prefix first so `/api/v2/users` beats `/api`.
        compiled.sort_by_key(|r| std::cmp::Reverse(r.prefix.len()));

        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            rules: compiled,
            client,
        }
    }

    fn matching(&self, path: &str) -> Option<&CompiledProxy> {
        self.rules.iter().find(|r| path.starts_with(&r.prefix))
    }
}

async fn proxy_mw(proxies: Arc<CompiledProxies>, req: Request<Body>, next: Next) -> Response<Body> {
    let path = req.uri().path().to_string();
    let Some(rule) = proxies.matching(&path) else {
        return next.run(req).await;
    };
    let rule = rule.clone();

    match proxy_forward(&proxies.client, &rule, req).await {
        Ok(resp) => resp,
        Err(e) => {
            tracing::warn!(?e, prefix = %rule.prefix, upstream = %rule.upstream, "proxy error");
            let mut resp = Response::new(Body::from(format!("upstream error: {e}")));
            *resp.status_mut() = StatusCode::BAD_GATEWAY;
            resp
        }
    }
}

async fn proxy_forward(
    client: &reqwest::Client,
    rule: &CompiledProxy,
    req: Request<Body>,
) -> Result<Response<Body>, String> {
    let (parts, body) = req.into_parts();
    let body_bytes = axum::body::to_bytes(body, usize::MAX)
        .await
        .map_err(|e| format!("body read: {e}"))?;

    let original_path = parts.uri.path();
    let upstream_path = if rule.strip_prefix {
        original_path
            .strip_prefix(&rule.prefix)
            .unwrap_or(original_path)
    } else {
        original_path
    };
    let upstream_path = if upstream_path.is_empty() {
        "/"
    } else {
        upstream_path
    };
    let query = parts
        .uri
        .query()
        .map(|q| format!("?{q}"))
        .unwrap_or_default();
    let upstream_url = format!("{}{}{}", rule.upstream, upstream_path, query);

    let method = parts.method.clone();
    let mut last_err = String::new();
    for attempt in 0..=rule.retries {
        let mut request = client
            .request(
                reqwest::Method::from_bytes(method.as_str().as_bytes())
                    .unwrap_or(reqwest::Method::GET),
                &upstream_url,
            )
            .timeout(rule.timeout)
            .body(body_bytes.clone());

        for (name, value) in parts.headers.iter() {
            // Hop-by-hop headers per RFC 7230 §6.1 — skip.
            let n = name.as_str().to_ascii_lowercase();
            if matches!(
                n.as_str(),
                "connection"
                    | "keep-alive"
                    | "proxy-authenticate"
                    | "proxy-authorization"
                    | "te"
                    | "trailers"
                    | "transfer-encoding"
                    | "upgrade"
                    | "content-length"
            ) {
                continue;
            }
            if !rule.preserve_host && n == "host" {
                continue;
            }
            if let Ok(v) = value.to_str() {
                request = request.header(name.as_str(), v);
            }
        }

        // X-Forwarded-* headers — useful for upstreams.
        if let Some(host) = parts.headers.get("host").and_then(|v| v.to_str().ok()) {
            request = request.header("x-forwarded-host", host);
        }
        request = request.header("x-forwarded-proto", "http");

        match request.send().await {
            Ok(resp) => return upstream_to_axum(resp).await,
            Err(e) => {
                last_err = format!("attempt {} → {e}", attempt + 1);
                tracing::debug!(error = %e, attempt, "proxy retry");
                continue;
            }
        }
    }
    Err(last_err)
}

// ─── CORS ───────────────────────────────────────────────────────────────────

async fn cors_mw(cfg: Arc<CorsConfig>, req: Request<Body>, next: Next) -> Response<Body> {
    let origin = req
        .headers()
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .map(String::from);

    let is_allowed_origin = origin.as_deref().is_some_and(|o| {
        cfg.allow_origins
            .iter()
            .any(|allowed| allowed == "*" || allowed == o)
    });

    // Preflight
    if req.method() == Method::OPTIONS && origin.is_some() {
        let mut resp = Response::new(Body::empty());
        *resp.status_mut() = StatusCode::NO_CONTENT;
        apply_cors_headers(
            resp.headers_mut(),
            &cfg,
            origin.as_deref(),
            is_allowed_origin,
        );
        return resp;
    }

    let mut resp = next.run(req).await;
    apply_cors_headers(
        resp.headers_mut(),
        &cfg,
        origin.as_deref(),
        is_allowed_origin,
    );
    resp
}

fn apply_cors_headers(
    headers: &mut HeaderMap,
    cfg: &CorsConfig,
    origin: Option<&str>,
    is_allowed_origin: bool,
) {
    if !is_allowed_origin {
        return;
    }
    if let Some(origin) = origin {
        if let Ok(v) = HeaderValue::from_str(origin) {
            headers.insert("access-control-allow-origin", v);
        }
        headers.insert("vary", HeaderValue::from_static("Origin"));
    } else if cfg.allow_origins.iter().any(|o| o == "*") {
        headers.insert("access-control-allow-origin", HeaderValue::from_static("*"));
    }

    let methods = if cfg.allow_methods.is_empty() {
        "GET, POST, PUT, PATCH, DELETE, OPTIONS".to_string()
    } else {
        cfg.allow_methods.join(", ")
    };
    if let Ok(v) = HeaderValue::from_str(&methods) {
        headers.insert("access-control-allow-methods", v);
    }

    let allow_headers = if cfg.allow_headers.is_empty() {
        "Content-Type, Authorization, X-CSRF-TOKEN, X-Requested-With".to_string()
    } else {
        cfg.allow_headers.join(", ")
    };
    if let Ok(v) = HeaderValue::from_str(&allow_headers) {
        headers.insert("access-control-allow-headers", v);
    }

    if !cfg.expose_headers.is_empty() {
        if let Ok(v) = HeaderValue::from_str(&cfg.expose_headers.join(", ")) {
            headers.insert("access-control-expose-headers", v);
        }
    }

    if cfg.allow_credentials {
        headers.insert(
            "access-control-allow-credentials",
            HeaderValue::from_static("true"),
        );
    }

    if let Some(max_age) = cfg.max_age {
        if let Ok(v) = HeaderValue::from_str(&max_age.as_secs().to_string()) {
            headers.insert("access-control-max-age", v);
        }
    }
}

// ─── IP allow/deny ──────────────────────────────────────────────────────────

async fn ip_rules_mw(
    rules: Arc<Vec<IpRule>>,
    trusted: Arc<Vec<ipnet::IpNet>>,
    req: Request<Body>,
    next: Next,
) -> Response<Body> {
    let path = req.uri().path().to_string();
    let ip_str = client_ip(&req, &trusted);
    let ip = ip_str.parse::<std::net::IpAddr>().ok();

    for rule in rules.iter() {
        if !path.starts_with(&rule.prefix) {
            continue;
        }
        let matches_range = ip
            .map(|addr| rule.ranges.iter().any(|net| net.contains(&addr)))
            .unwrap_or(false);
        let allowed = match rule.action {
            IpAction::Allow => matches_range,
            IpAction::Deny => !matches_range,
        };
        if !allowed {
            tracing::debug!(path, ip = %ip_str, "ip rule denied request");
            let mut resp = Response::new(Body::from("forbidden"));
            *resp.status_mut() = StatusCode::FORBIDDEN;
            return resp;
        }
        // First matching prefix wins.
        break;
    }

    next.run(req).await
}

// ─── HTTP Basic Auth ────────────────────────────────────────────────────────

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;

struct CompiledBasicAuth {
    rules: Vec<(BasicAuthRule, Vec<(String, String)>)>,
}

fn compile_basic_auth(rules: &[BasicAuthRule]) -> CompiledBasicAuth {
    let compiled = rules
        .iter()
        .map(|r| {
            let creds = r
                .credentials
                .iter()
                .filter_map(|c| {
                    c.split_once(':')
                        .map(|(u, p)| (u.to_string(), p.to_string()))
                })
                .collect();
            (r.clone(), creds)
        })
        .collect();
    CompiledBasicAuth { rules: compiled }
}

async fn basic_auth_mw(
    rules: Arc<CompiledBasicAuth>,
    req: Request<Body>,
    next: Next,
) -> Response<Body> {
    let path = req.uri().path().to_string();
    for (rule, creds) in &rules.rules {
        if !path.starts_with(&rule.prefix) {
            continue;
        }
        let supplied = req
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Basic "))
            .and_then(|b64| B64.decode(b64).ok())
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .and_then(|pair| {
                pair.split_once(':')
                    .map(|(u, p)| (u.to_string(), p.to_string()))
            });

        let ok = supplied
            .as_ref()
            .map(|(u, p)| creds.iter().any(|(cu, cp)| cu == u && cp == p))
            .unwrap_or(false);

        if ok {
            return next.run(req).await;
        }

        let challenge = format!("Basic realm=\"{}\"", rule.realm);
        let mut resp = Response::new(Body::from("authentication required"));
        *resp.status_mut() = StatusCode::UNAUTHORIZED;
        if let Ok(v) = HeaderValue::from_str(&challenge) {
            resp.headers_mut().insert("www-authenticate", v);
        }
        return resp;
    }
    next.run(req).await
}

async fn upstream_to_axum(resp: reqwest::Response) -> Result<Response<Body>, String> {
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("upstream body: {e}"))?;
    let mut out = Response::new(Body::from(bytes));
    *out.status_mut() =
        axum::http::StatusCode::from_u16(status.as_u16()).unwrap_or(axum::http::StatusCode::OK);
    for (name, value) in headers.iter() {
        let n = name.as_str().to_ascii_lowercase();
        if matches!(
            n.as_str(),
            "connection"
                | "keep-alive"
                | "proxy-authenticate"
                | "proxy-authorization"
                | "te"
                | "trailers"
                | "transfer-encoding"
                | "upgrade"
        ) {
            continue;
        }
        if let Ok(v) = HeaderValue::from_bytes(value.as_bytes()) {
            if let Ok(name) = HeaderName::from_bytes(name.as_str().as_bytes()) {
                out.headers_mut().append(name, v);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn make_req(peer: Option<SocketAddr>, xff: Option<&str>) -> Request<Body> {
        let mut req = Request::builder();
        if let Some(v) = xff {
            req = req.header("x-forwarded-for", v);
        }
        let mut req = req.body(Body::empty()).unwrap();
        if let Some(addr) = peer {
            req.extensions_mut().insert(ConnectInfo(addr));
        }
        req
    }

    fn cidr(s: &str) -> ipnet::IpNet {
        s.parse().unwrap()
    }

    #[test]
    fn xff_ignored_when_peer_is_not_trusted() {
        // Hostile client direct-connects and sets their own XFF — must be ignored.
        let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 5)), 12345);
        let req = make_req(Some(peer), Some("198.51.100.1"));
        let trusted = vec![cidr("10.0.0.0/8")];

        assert_eq!(client_ip(&req, &trusted), "203.0.113.5");
    }

    #[test]
    fn xff_honored_when_peer_is_a_trusted_proxy() {
        // The LB at 10.0.0.5 forwards the original client's IP.
        let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)), 443);
        let req = make_req(Some(peer), Some("198.51.100.1, 10.0.0.5"));
        let trusted = vec![cidr("10.0.0.0/8")];

        assert_eq!(client_ip(&req, &trusted), "198.51.100.1");
    }

    #[test]
    fn empty_trusted_list_means_xff_is_never_honored() {
        // Even XFF from a private peer is ignored when no trusted ranges configured.
        let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)), 443);
        let req = make_req(Some(peer), Some("198.51.100.1"));
        let trusted: Vec<ipnet::IpNet> = vec![];

        assert_eq!(client_ip(&req, &trusted), "10.0.0.5");
    }

    #[test]
    fn no_xff_falls_back_to_peer() {
        let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)), 443);
        let req = make_req(Some(peer), None);
        let trusted = vec![cidr("10.0.0.0/8")];

        assert_eq!(client_ip(&req, &trusted), "10.0.0.5");
    }

    #[test]
    fn missing_connect_info_returns_unknown() {
        // Defensive: a router built without `into_make_service_with_connect_info`
        // won't have the extension; we shouldn't panic, and we shouldn't trust XFF.
        let req = make_req(None, Some("198.51.100.1"));
        let trusted = vec![cidr("10.0.0.0/8")];

        assert_eq!(client_ip(&req, &trusted), "unknown");
    }

    #[test]
    fn xff_with_whitespace_and_multiple_hops_picks_first() {
        let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)), 443);
        let req = make_req(Some(peer), Some("  198.51.100.1 ,10.0.0.5, 10.0.0.7"));
        let trusted = vec![cidr("10.0.0.0/8")];

        assert_eq!(client_ip(&req, &trusted), "198.51.100.1");
    }
}
