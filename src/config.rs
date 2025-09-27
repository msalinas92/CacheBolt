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

use once_cell::sync::OnceCell;
use serde::Deserialize;
use std::{collections::HashSet, error::Error, fs};

/// Supported persistent storage backends for the cache.
#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(rename_all = "lowercase")]
pub enum StorageBackend {
    Gcs,
    S3,
    Azure,
    Local,
}

/// Cache-related settings for memory usage and re-cache policies.
#[derive(Debug, Deserialize, Clone)]
pub struct CacheSettings {
    /// Memory usage threshold as a percentage (e.g., 80 = 80%).
    pub memory_threshold: usize,

    /// Percentage of fallback requests that should attempt revalidation.
    #[serde(default)]
    pub refresh_percentage: u8,

    /// Time-to-live (TTL) for cached responses in seconds.
    #[serde(default)]
    pub ttl_seconds: u64,
}

/// Describes latency thresholds per path to decide when to fallback to the cache.
#[derive(Debug, Deserialize, Clone)]
pub struct MaxLatencyRule {
    /// Regex pattern to match request paths (e.g., ^/api/products).
    pub pattern: String,

    /// Maximum allowable response time in milliseconds for this pattern.
    pub max_latency_ms: u64,
}

/// Fallback configuration based on request latency.
#[derive(Debug, Deserialize, Clone)]
pub struct LatencyFailover {
    /// Default latency limit in milliseconds if no rule matches.
    pub default_max_latency_ms: u64,

    /// Specific path-based rules, applied in order.
    #[serde(default)] // <--- Esto lo hace opcional en YAML y por defecto = []
    pub path_rules: Vec<MaxLatencyRule>,
}

/// Main configuration structure loaded from a YAML file.
/// Defines all tunable behavior of the application.
#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    /// Application identifier, used for namespacing cache keys or logs.
    pub app_id: String,

    /// GCS bucket name (used if storage_backend is set to GCS).
    pub gcs_bucket: String,

    /// AWS S3 bucket name.
    pub s3_bucket: String,

    /// Azure Blob Storage container name.
    pub azure_container: String,

    /// Max number of concurrent requests allowed by the proxy.
    pub max_concurrent_requests: usize,

    /// Base URL of the downstream service that CacheBolt proxies.
    pub downstream_base_url: String,

    /// Cache settings including memory limits and re-cache rules.
    pub cache: CacheSettings,

    /// Latency-based failover rules.
    pub latency_failover: LatencyFailover,

    /// Backend to use for persistent cache storage.
    pub storage_backend: StorageBackend,

    /// Number of allowed failures for a storage backend before treating it as unhealthy.
    /// Must be a positive integer (0 is allowed to disable the circuit breaker).
    pub storage_backend_failures: usize,

    /// Retry interval (in seconds) to wait before retrying an unhealthy backend.
    /// Must be a non-negative integer (0 disables retries).
    pub backend_retry_interval_secs: u64,

    /// Headers to ignore when computing cache keys.
    pub ignored_headers: Option<Vec<String>>,

    /// Port for proxy traffic (default: 3000).
    #[serde(default = "default_proxy_port")]
    pub proxy_port: u16,

    /// Port for admin UI and Prometheus metrics (default: 3001).
    #[serde(default = "default_admin_port")]
    pub admin_port: u16,
}

/// Default port for proxy service
fn default_proxy_port() -> u16 {
    3000
}

/// Default port for admin + metrics service
fn default_admin_port() -> u16 {
    3001
}


/// Global, lazily-initialized config object shared across the application.
pub static CONFIG: OnceCell<Config> = OnceCell::new();

impl Config {
    /// Parses configuration from a YAML file.
    ///
    /// # Arguments
    /// - `path`: File path to the config YAML (e.g., "config.yaml").
    ///
    /// # Returns
    /// - `Ok(Config)` if parsing is successful.
    /// - `Err(Box<dyn Error>)` if the file is missing, malformed, or invalid.
    pub fn from_file(path: &str) -> Result<Self, Box<dyn Error>> {
        // Load the file contents as a string
        let contents = fs::read_to_string(path)?;
        // Deserialize YAML into the Config struct
        let parsed: Config = serde_yaml::from_str(&contents)?;

        // Validate required fields based on selected backend
        match parsed.storage_backend {
            StorageBackend::Gcs if parsed.gcs_bucket.trim().is_empty() => {
                return Err("GCS backend selected but gcs_bucket is empty.".into());
            }
            StorageBackend::S3 if parsed.s3_bucket.trim().is_empty() => {
                return Err("S3 backend selected but s3_bucket is empty.".into());
            }
            StorageBackend::Azure if parsed.azure_container.trim().is_empty() => {
                return Err("Azure backend selected but azure_container is empty.".into());
            }
            _ => {}
        }

        // Validate app_id
        if parsed.app_id.trim().is_empty() {
            return Err("app_id is required and cannot be empty.".into());
        }

        // Validate memory threshold
        if parsed.cache.memory_threshold == 0 || parsed.cache.memory_threshold > 100 {
            return Err("cache.memory_threshold must be between 1 and 100.".into());
        }

        // Log latency failover rules
        if parsed.latency_failover.path_rules.is_empty() {
            tracing::info!(
                "No per-path latency rules defined. Using default max latency: {}ms",
                parsed.latency_failover.default_max_latency_ms
            );
        } else {
            for rule in &parsed.latency_failover.path_rules {
                tracing::info!(
                    "Latency rule: pattern = '{}', max_latency = {}ms",
                    rule.pattern,
                    rule.max_latency_ms
                );
            }
        }

        Ok(parsed)
    }

    /// Returns the list of headers to ignore (lowercased).
    pub fn ignored_headers_set(&self) -> HashSet<String> {
        let mut ignored = self
            .ignored_headers
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|h| h.to_ascii_lowercase())
            .collect::<HashSet<_>>();

        // Add bypass and refresh headers as default ignored
        ignored.insert("x-bypass-cache".to_string());
        ignored.insert("x-refresh-cache".to_string());
        ignored.insert("cache-control".to_string());

        ignored
    }
}
