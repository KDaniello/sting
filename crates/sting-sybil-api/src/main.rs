//! The terminal surface over the request path: load a model and a cache, then serve.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use axum::error_handling::HandleErrorLayer;
use axum::http::StatusCode;
use clap::{Parser, Subcommand};
use sting_sybil_api::{ApiError, AppState, Result, auth, feature_cache, metrics, router_with_auth};
use tower::ServiceBuilder;

#[derive(Parser)]
#[command(name = "sting-sybil-api")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Serves scoring over HTTP.
    ///
    /// Everything is loaded before the port opens: the model, the warm cache, and the keys. A
    /// service that discovered a missing file on its first request would answer that request
    /// wrongly rather than not starting.
    Serve {
        /// The model file the training side wrote.
        #[arg(long)]
        linear_scorer_path: String,
        /// Where the feature partitions are.
        #[arg(long, default_value = "features")]
        features_root: String,
        /// Where the membership partitions are.
        #[arg(long, default_value = "cluster")]
        cluster_root: String,
        /// Which vocabulary version to serve.
        #[arg(long)]
        feature_version: String,
        /// The keys and tiers that may ask.
        #[arg(long)]
        api_keys_path: String,
        /// Which port to listen on.
        #[arg(long, default_value_t = 8080)]
        port: u16,
        /// How many requests may be in flight at once before the rest are shed.
        #[arg(long, default_value_t = 256)]
        max_concurrency: usize,
        /// How long a request may take before it is abandoned.
        #[arg(long, default_value_t = 200)]
        timeout_ms: u64,
    },
}

/// Turns a middleware failure into the response it deserves.
///
/// The router requires a service that cannot fail, so timeouts and shed load have to become
/// answers somewhere: here, where a timeout is a timeout and overload is overload rather than
/// both being an internal error.
async fn handle_middleware_error(err: tower::BoxError) -> (StatusCode, String) {
    if err.is::<tower::timeout::error::Elapsed>() {
        (StatusCode::REQUEST_TIMEOUT, "request timed out".to_string())
    } else if err.is::<tower::load_shed::error::Overloaded>() {
        (
            StatusCode::TOO_MANY_REQUESTS,
            "server overloaded, try again later".to_string(),
        )
    } else {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("unhandled middleware error: {err}"),
        )
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    match cli.command {
        Command::Serve {
            linear_scorer_path,
            features_root,
            cluster_root,
            feature_version,
            api_keys_path,
            port,
            max_concurrency,
            timeout_ms,
        } => {
            let scorer_toml = std::fs::read_to_string(&linear_scorer_path)?;
            let scorer = sting_sybil_scorer::LinearScorer::from_toml_str(&scorer_toml).map_err(
                |source| ApiError::Config {
                    path: linear_scorer_path.clone(),
                    source,
                },
            )?;

            let feature_cache_map = feature_cache::load_feature_cache(
                Path::new(&features_root),
                Path::new(&cluster_root),
                &feature_version,
            )?;
            // Through the subscriber this binary just installed rather than around it: two ways
            // of saying what a service is doing is one of them nobody is collecting.
            tracing::info!(
                addresses = feature_cache_map.len(),
                version = %feature_version,
                "warm cache loaded"
            );

            let api_keys_toml = std::fs::read_to_string(&api_keys_path)?;
            let api_keys = auth::ApiKeyStore::from_toml_str(&api_keys_toml).map_err(|source| {
                ApiError::Config {
                    path: api_keys_path.clone(),
                    source,
                }
            })?;
            let auth_state = Arc::new(auth::AuthState {
                api_keys,
                rate_limiter: auth::RateLimiter::new(),
            });

            let state = Arc::new(AppState {
                scorer,
                feature_cache: feature_cache_map,
                metrics: metrics::Metrics::new(),
            });

            let middleware = ServiceBuilder::new()
                .layer(HandleErrorLayer::new(handle_middleware_error))
                .load_shed()
                .concurrency_limit(max_concurrency)
                .timeout(Duration::from_millis(timeout_ms));

            let app = router_with_auth(state, auth_state)
                .layer(tower_http::trace::TraceLayer::new_for_http())
                .layer(middleware);

            let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
            tracing::info!(port, "listening");
            axum::serve(listener, app).await?;
        }
    }
    Ok(())
}
