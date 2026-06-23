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

    let session_layer = SessionManagerLayer::new(store)
        .with_secure(false) // allow over plain HTTP for LAN use; set true behind TLS
        .with_same_site(SameSite::Lax)
        .with_http_only(true)
        .with_expiry(Expiry::OnInactivity(CookieDuration::seconds(
            config.session_ttl_secs,
        )))
        .with_name("webssh_session");

    let state = AppState {
        pool,
        config: std::sync::Arc::new(config.clone()),
        sftp: std::sync::Arc::new(tokio::sync::RwLock::new(Default::default())),
    };

    let app = build_router(state).layer(session_layer);

    let addr: SocketAddr = format!("{}:{}", config.host, config.port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

/// Compose the full router: authenticated `/api/*` plus embedded SPA fallback.
fn build_router(state: AppState) -> Router {
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

    Router::new()
        .nest("/api", api)
        .route("/healthz", get(|| async { "ok" }))
        // SPA: serve embedded assets, fall back to index.html for client routes.
        .fallback(static_handler)
        .with_state(state)
}

/// Serve an embedded frontend asset, or `index.html` for unknown paths.
async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    // Try the exact file first.
    if let Some(asset) = FrontendAsset::get(path) {
        return asset_response(path, asset);
    }
    // SPA fallback.
    match FrontendAsset::get("index.html") {
        Some(index) => asset_response("index.html", index),
        None => (
            StatusCode::NOT_FOUND,
            "frontend not built (run `npm run build` in frontend/)",
        )
            .into_response(),
    }
}

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
