//! web-ssh backend entry point.
//!
//! Wires the SQLite pool, encrypted session store, and route modules into a
//! single axum server. In release builds the compiled Vue frontend is embedded
//! and served from `/`; in dev the frontend runs on Vite's own port and proxies
//! `/api` here.

mod auth;
mod captcha;
mod config;
mod connections;
mod crypto;
mod db;
mod error;
mod files;
mod ssh;
mod state;
mod terminal;
mod users;

use std::net::SocketAddr;

use axum::{
    Router,
    http::{HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use rust_embed::RustEmbed;
use tower_sessions::{
    Expiry, SessionManagerLayer,
    cookie::{SameSite, time::Duration as CookieDuration},
};
use tower_sessions_rusqlite_store::RusqliteStore;

use crate::state::AppState;

#[derive(RustEmbed)]
#[folder = "../frontend/dist"]
struct FrontendAsset;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,web_ssh_backend=debug".into()),
        )
        .init();

    let config = config::Config::load()?;
    tracing::info!(host = %config.host, port = config.port, "starting web-ssh");

    // Database.
    let pool = db::create_pool(&config.db_path)?;

    // Session store backed by the same SQLite file via an async tokio-rusqlite
    // connection. WAL allows the session store and the deadpool app pool to use
    // the same file concurrently.
    let session_conn =
        tower_sessions_rusqlite_store::tokio_rusqlite::Connection::open(&config.db_path).await?;
    let store = RusqliteStore::new(session_conn);
    store.migrate().await?;

    // Cookie path: scoped to the context path (or "/" for root deployment) so
    // the session cookie isn't sent to other apps on the same host.
    let cookie_path = if config.context_path.is_empty() {
        "/".to_string()
    } else {
        format!("{}/", config.context_path)
    };
    let session_layer = SessionManagerLayer::new(store)
        .with_secure(false) // allow over plain HTTP for LAN use; set true behind TLS
        .with_same_site(SameSite::Lax)
        .with_http_only(true)
        .with_path(cookie_path)
        .with_expiry(Expiry::OnInactivity(CookieDuration::seconds(
            config.session_ttl_secs,
        )))
        .with_name("webssh_session");

    let context_path = config.context_path.clone();
    let state = AppState {
        pool,
        config: std::sync::Arc::new(config.clone()),
        sftp: std::sync::Arc::new(tokio::sync::RwLock::new(Default::default())),
    };

    let app = build_router(state, &context_path).layer(session_layer);

    let addr: SocketAddr = format!("{}:{}", config.host, config.port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

/// Compose the full router. When `context_path` is non-empty (e.g. "/webssh"),
/// the whole app — `/api`, `/healthz`, and the SPA — is nested under it, and a
/// bare `/` redirects to `{context_path}/` for convenience. An empty
/// `context_path` keeps the original root deployment.
fn build_router(state: AppState, context_path: &str) -> Router {
    let api = Router::new()
        .route("/auth/status", get(auth::status)) // public, no auth required
        .nest("/auth", auth::router())
        .nest("/connections", connections::router())
        .nest("/users", users::router())
        .route("/files/list", get(files::list))
        .route("/files/download", get(files::download))
        .route("/files/upload", post(files::upload))
        .route("/files/mkdir", post(files::mkdir))
        .route("/files", axum::routing::delete(files::remove))
        .route("/terminal/connect", get(terminal::connect))
        .route("/terminal/probe", get(terminal::probe));

    let ctx = context_path.to_string();
    let inner = Router::new()
        .nest("/api", api)
        .route("/healthz", get(|| async { "ok" }))
        // SPA: serve embedded assets, fall back to index.html for client routes.
        .fallback(move |uri: Uri| static_handler(uri, ctx.clone()))
        .with_state(state);

    if context_path.is_empty() {
        inner
    } else {
        // Redirect "/" → "{context_path}" for bare-host convenience.
        let root_dest = context_path.to_string();
        // Also redirect "{context_path}/" → "{context_path}". axum's
        // `.nest("/p", …)` quirk makes the exact "/p/" miss the inner fallback
        // (404) while "/p" serves the SPA; normalizing to no-slash sidesteps it.
        let slash_route = format!("{context_path}/");
        let slash_dest = context_path.to_string();
        Router::new()
            .route("/", get(move || async move { redirect_to(root_dest.clone()) }))
            .route(
                &slash_route,
                get(move || async move { redirect_to(slash_dest.clone()) }),
            )
            .nest(context_path, inner)
    }
}

/// 302 redirect helper for the root → context-path convenience redirect.
fn redirect_to(location: String) -> Response {
    let mut resp = StatusCode::FOUND.into_response();
    resp.headers_mut().insert(
        axum::http::header::LOCATION,
        HeaderValue::from_str(&location).unwrap_or(HeaderValue::from_static("/")),
    );
    resp
}

/// Serve an embedded frontend asset, or `index.html` for unknown paths. The
/// `context_path` is stripped before the asset lookup, and `index.html` gets a
/// `<base href="{ctx}/">` + `window.__CONTEXT_PATH__` injected so the SPA works
/// under a sub-path.
async fn static_handler(uri: Uri, context_path: String) -> Response {
    let mut path = uri.path();
    // Strip the context path prefix so lookups are relative to the app root.
    if !context_path.is_empty() {
        path = path.strip_prefix(context_path.as_str()).unwrap_or(path);
    }
    let path = path.trim_start_matches('/');
    // Try the exact file first.
    if let Some(asset) = FrontendAsset::get(path) {
        return asset_response(path, asset);
    }
    // SPA fallback: inject context-path awareness into index.html.
    match FrontendAsset::get("index.html") {
        Some(index) => index_response(&index.data, &context_path),
        None => (
            StatusCode::NOT_FOUND,
            "frontend not built (run `npm run build` in frontend/)",
        )
            .into_response(),
    }
}

/// Build a response for a non-HTML asset.
fn asset_response(path: &str, asset: rust_embed::EmbeddedFile) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let body = axum::body::Body::from(asset.data.into_owned());
    let mut resp = Response::new(body);
    resp.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_str(mime.essence_str()).unwrap(),
    );
    if path != "index.html" {
        // Hashed Vite assets are safe to cache aggressively.
        resp.headers_mut().insert(
            axum::http::header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        );
    }
    resp
}

/// Serve `index.html` with a `<base href="{ctx}/">` (so the browser resolves
/// relative asset URLs under the sub-path) and a `window.__CONTEXT_PATH__`
/// global (so the SPA builds API/WS/router URLs with the right prefix).
/// `ctx` is "" for root deployment.
fn index_response(raw: &[u8], ctx: &str) -> Response {
    let html = String::from_utf8_lossy(raw);
    let base_href = if ctx.is_empty() {
        "/".to_string()
    } else {
        format!("{ctx}/")
    };
    // Use serde_json to produce a properly-quoted & escaped JS string, then
    // strip the outer JSON quotes so we can embed it in a <script> tag.
    let js_ctx = serde_json::to_string(ctx).unwrap_or_else(|_| "\"\"".into());
    let injection = format!(
        "<base href=\"{base_href}\"><script>window.__CONTEXT_PATH__={js_ctx};</script>"
    );
    // Inject right after <head> so the <base> takes effect before other assets.
    let out = if let Some(idx) = html.find("<head>") {
        let (a, b) = html.split_at(idx + "<head>".len());
        format!("{a}{injection}{b}")
    } else {
        format!("{injection}{html}")
    };
    let mut resp = Response::new(axum::body::Body::from(out));
    resp.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    // Never cache index.html — its injected base/global change with config.
    resp.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache, no-store, must-revalidate"),
    );
    resp
}
