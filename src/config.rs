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
use std::{fs, error::Error};

/// Supported persistent storage backends for the cache.
/// This enum is deserialized from lowercase strings in the YAML config.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StorageBackend {
    Gcs,
    S3,
    Azure,
    Local,
}

/// Configuration for memory-based eviction strategy.
/// Eviction triggers when system memory usage exceeds a certain percentage.
#[derive(Debug, Deserialize)]
pub struct MemoryEviction {
    /// Memory usage threshold as a percentage (e.g., 80 = 80%).
    pub threshold_percent: usize,
}

/// Describes latency thresholds per path to decide when to fallback to the cache.
/// Useful for protecting the system when downstream responses become too slow.
#[derive(Debug, Deserialize)]
pub struct MaxLatencyRule {
    /// Regex pattern to match request paths (e.g., ^/api/products).
    pub pattern: String,
    /// Maximum allowable response time in milliseconds for this pattern.
    pub max_latency_ms: u64,
}

/// Fallback configuration based on request latency.
#[derive(Debug, Deserialize)]
pub struct LatencyFailover {
    /// Default latency limit in milliseconds if no rule matches.
    pub default_max_latency_ms: u64,
    /// Specific path-based rules, applied in order.
    pub path_rules: Vec<MaxLatencyRule>,
}

/// Main configuration structure loaded from a YAML file.
/// Defines all tunable behavior of the application.
#[derive(Debug, Deserialize)]
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

    /// Timeout for downstream requests in seconds.
    pub downstream_timeout_secs: u64,

    /// Memory eviction policy settings.
    pub memory_eviction: MemoryEviction,

    /// Latency-based failover rules.
    pub latency_failover: LatencyFailover,

    /// Backend to use for persistent cache storage.
    pub storage_backend: StorageBackend,
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
            _ => {}
        }

        // Provide info logs about latency fallback rules
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
}
