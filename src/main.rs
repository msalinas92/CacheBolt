// Copyright (C) 2025 Matías Salinas (support@fenden.com)
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// ----------------------
//  Module declarations
// ----------------------
// These are internal modules for handling the proxy logic, caching layers,
// configuration loading, and in-memory eviction based on memory pressure.
mod proxy;
mod storage;
mod memory;
mod config;
mod eviction;
mod rules;

// ----------------------
// External dependencies
// ----------------------
use axum::{routing::get, Router};               // Axum: Web framework for routing and request handling
use hyper::Server;                              // Hyper: High-performance HTTP server
use std::{net::SocketAddr, process::exit};      // Network + system utilities

use clap::Parser;                               // CLI argument parsing (via `--config`)
use tracing::{info, warn, error};               // Structured logging macros
use tracing_subscriber::EnvFilter;              // Log filtering via LOG_LEVEL

// ----------------------
// Internal dependencies
// ----------------------
use crate::config::{Config, CONFIG, StorageBackend};             // App-wide config definitions
use crate::eviction::start_background_eviction_task;             // Memory pressure eviction
use crate::storage::{gcs, s3, azure};                             // Persistent storage backends

/// ----------------------------
/// CLI ARGUMENT STRUCTURE
/// ----------------------------
/// Defines CLI arguments that can be passed to the binary,
/// such as the path to the configuration file.
/// Defaults to "config.yaml" if not provided.
#[derive(Parser, Debug)]
#[command(
    name = "CacheBolt",
    version = "0.1.0",
    author = "Matías Salinas Contreras <support@fenden.com>",
    about = "Intelligent reverse proxy with in-memory and multi-cloud caching",
    long_about = Some(
        "CacheBolt is a high-performance reverse proxy with \
        in-memory and multi-cloud caching support.\n\n\
        Author: Matías Salinas Contreras <support@fenden.com>\n\
        Version: 0.1.0"
    )
)]
struct Args {
    /// Path to the YAML configuration file
    #[arg(long, default_value = "config.yaml")]
    config: String,
}

/// ----------------------------
/// LOGGING INITIALIZATION
/// ----------------------------
/// Initializes structured logging using the `LOG_LEVEL` environment variable.
/// Falls back to "info" if not set. Avoids using `RUST_LOG` to provide
/// a more consistent developer experience.

fn init_logging(app_id: &str) {
    let filter = EnvFilter::try_new(std::env::var("LOG_LEVEL").unwrap_or_else(|_| "info".into()))
        .unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)   // Uses LOG_LEVEL to filter verbosity
        .with_target(false)        // Hides the module path in each log line
        .compact()                 // Compact single-line logs (less verbose)
        .init();

    info!("🚀 Logging initialized for app_id: {app_id}");
}

/// -----------------------------------------
/// BACKEND INITIALIZATION DISPATCHER
/// -----------------------------------------
/// Based on the `storage_backend` defined in the loaded config,
/// initializes the appropriate persistent cache client.
/// Supports: GCS, S3, Azure Blob, and Local (no-op).

async fn init_selected_backend() {
    match CONFIG.get().map(|c| &c.storage_backend) {
        Some(StorageBackend::Gcs) => {
            // Initializes Google Cloud Storage client (authenticated via ADC or env vars)
            let gcs_config = google_cloud_storage::client::ClientConfig::default()
                .with_auth()
                .await
                .expect("❌ Failed to authenticate with GCS");

            let client = google_cloud_storage::client::Client::new(gcs_config);
            if gcs::GCS_CLIENT.set(client).is_err() {
                warn!("⚠️ GCS_CLIENT was already initialized");
            } else {
                info!("✅ GCS client initialized successfully");
            }
        }
        Some(StorageBackend::S3) => {
            // Initializes AWS S3 client using env vars: AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, AWS_REGION
            s3::init_s3_client().await;
            info!("✅ AWS S3 client initialized successfully");
        }
        Some(StorageBackend::Azure) => {
            // Initializes Azure Blob Storage using env vars: AZURE_STORAGE_ACCOUNT, AZURE_STORAGE_ACCESS_KEY
            azure::init_azure_client();
            info!("✅ Azure Blob client initialized successfully");
        }
        Some(StorageBackend::Local) => {
            // No initialization needed for local file-based caching
            info!("🗄 Local storage backend selected (no setup required).");
        }
        None => {
            error!("❌ No storage backend configured. Terminating execution.");
            exit(1);
        }
    }
}

/// ---------------------------
/// APPLICATION ENTRY POINT
/// ---------------------------
/// Starts the reverse proxy server using Axum and initializes all required components.
/// Includes config loading, backend setup, memory cache eviction, and HTTP server launch.
#[tokio::main(flavor = "multi_thread", worker_threads = 4)]

async fn main() {
    // ------------------------------------------------------
    // 1. Parse CLI arguments (e.g., --config=config.prod.yaml)
    // ------------------------------------------------------
    let args = Args::parse();

    // ------------------------------------------------------
    // 2. Load configuration from YAML file
    // ------------------------------------------------------
    let config = match Config::from_file(&args.config) {
        Ok(cfg) => cfg,
        Err(e) => {
            error!("❌ Failed to load config from '{}': {e}", args.config);
            exit(1);
        }
    };

    // ------------------------------------------------------
    // 3. Initialize the logger using app_id for context
    // ------------------------------------------------------
    init_logging(&config.app_id);

    // ------------------------------------------------------
    // 4. Set global CONFIG (OnceCell) for use across modules
    // ------------------------------------------------------
    CONFIG.set(config).expect("❌ CONFIG was already initialized");

    // ------------------------------------------------------
    // 5. Initialize persistent storage backend (GCS, S3, Azure, Local)
    // ------------------------------------------------------
    init_selected_backend().await;

    // ------------------------------------------------------
    // 6. Start the background memory eviction task
    //    This task monitors system memory usage and evicts
    //    in-memory cache entries when usage exceeds threshold.
    // ------------------------------------------------------
    start_background_eviction_task();

    // ------------------------------------------------------
    // 7. Define Axum router with a single wildcard route
    //    All incoming GET requests will be handled by the proxy logic.
    // ------------------------------------------------------
    let app = Router::new().route("/*path", get(proxy::proxy_handler));

    // ------------------------------------------------------
    // 8. Bind the server to all interfaces on port 3000
    // ------------------------------------------------------
    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    info!("🚀 Server listening at http://{}", addr);

    // ------------------------------------------------------
    // 9. Start serving HTTP requests using Axum and Hyper
    // ------------------------------------------------------
    Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}