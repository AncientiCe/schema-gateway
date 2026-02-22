//! File watcher for config and schema hot-reloading.

use crate::config::Config;
use crate::handler::AppState;
use crate::openapi::OpenApiCache;
use crate::schema::SchemaCache;
use notify::RecursiveMode;
use notify_debouncer_mini::new_debouncer;
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;
use std::{sync::Arc, thread};
use tokio::sync::{mpsc as tokio_mpsc, RwLock};

/// Handle to the file watcher. Drop to stop watching and allow clean shutdown.
pub struct WatcherHandle {
    _shutdown_tx: mpsc::Sender<()>,
}

/// Start watching the config file and all referenced schema/spec paths.
/// On change (debounced 500ms), reloads config and caches and swaps into `state`.
/// Returns a handle; drop it to stop the watcher.
pub fn start_watcher(
    config_path: std::path::PathBuf,
    state: Arc<RwLock<AppState>>,
) -> WatcherHandle {
    let (reload_tx, mut reload_rx) = tokio_mpsc::channel::<()>(4);
    let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();

    let config_path_for_thread = config_path.clone();
    let reload_tx_sync = Arc::new(reload_tx);

    thread::spawn(move || {
        let config = match Config::from_file(&config_path_for_thread) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("watcher: could not load config to discover paths: {}", e);
                return;
            }
        };

        let paths = config.watched_paths(&config_path_for_thread);
        let mut debouncer = match new_debouncer(Duration::from_millis(500), move |_res| {
            let _ = reload_tx_sync.try_send(());
        }) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("watcher: could not create debouncer: {}", e);
                return;
            }
        };

        for path in &paths {
            if path.exists() {
                if path.is_file() {
                    if let Err(e) = debouncer.watcher().watch(path, RecursiveMode::NonRecursive) {
                        tracing::debug!("watcher: could not watch {}: {}", path.display(), e);
                    }
                } else if let Err(e) = debouncer.watcher().watch(path, RecursiveMode::NonRecursive)
                {
                    tracing::debug!("watcher: could not watch dir {}: {}", path.display(), e);
                }
            } else if let Some(parent) = path.parent() {
                if parent.exists() {
                    if let Err(e) = debouncer
                        .watcher()
                        .watch(parent, RecursiveMode::NonRecursive)
                    {
                        tracing::debug!(
                            "watcher: could not watch parent {}: {}",
                            parent.display(),
                            e
                        );
                    }
                }
            }
        }

        tracing::info!("watching {} path(s) for config/schema changes", paths.len());

        let _ = shutdown_rx.recv();
        drop(debouncer);
    });

    tokio::spawn(async move {
        while reload_rx.recv().await.is_some() {
            if let Err(e) = try_reload(&config_path, &state).await {
                tracing::error!("hot-reload failed: {}", e);
            }
        }
    });

    WatcherHandle {
        _shutdown_tx: shutdown_tx,
    }
}

async fn try_reload(config_path: &Path, state: &Arc<RwLock<AppState>>) -> Result<(), String> {
    let config = Config::from_file(config_path)?;
    config.validate()?;

    let schema_paths: Vec<_> = config
        .routes
        .iter()
        .filter_map(|r| r.schema.clone())
        .collect();
    let mut schema_cache = SchemaCache::new();
    let schema_errors = schema_cache.preload_all(schema_paths.iter());
    if !schema_errors.is_empty() {
        for (path, err) in &schema_errors {
            tracing::warn!(
                "hot-reload: failed to load schema {}: {}",
                path.display(),
                err
            );
        }
        return Err(format!("{} schema(s) failed to load", schema_errors.len()));
    }

    let mut openapi_cache = OpenApiCache::new();
    let openapi_errors = openapi_cache.preload_routes(&config.routes);
    if !openapi_errors.is_empty() {
        for (route, err) in &openapi_errors {
            tracing::warn!("hot-reload: failed to load OpenAPI {}: {}", route, err);
        }
        return Err(format!(
            "{} OpenAPI operation(s) failed to load",
            openapi_errors.len()
        ));
    }

    let mut guard = state.write().await;
    guard.config = config;
    guard.schema_cache = Arc::new(tokio::sync::RwLock::new(schema_cache));
    guard.openapi_cache = Arc::new(tokio::sync::RwLock::new(openapi_cache));
    drop(guard);

    tracing::info!("config and schemas reloaded successfully");
    Ok(())
}
