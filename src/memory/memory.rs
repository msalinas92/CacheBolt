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

use crate::config::CONFIG;
use bytes::Bytes;
use lru::LruCache;
use once_cell::sync::Lazy;
use std::collections::hash_map::RandomState;
use std::sync::Arc;
use sysinfo::System;
use tokio::sync::RwLock;
use tracing::info;
use chrono::{DateTime, Utc}; 

/// Structure representing an HTTP response cached in memory.
/// This includes the full response body and a simplified list of headers.
#[derive(Clone)]
pub struct CachedResponse {
    pub body: Bytes,
    pub headers: Vec<(String, String)>,
    #[allow(dead_code)]
    pub inserted_at: DateTime<Utc>,
}

/// Type alias for the thread-safe, shared in-memory cache structure.
/// It uses Tokio's `RwLock` and an `Arc` to enable concurrent reads and mutation across tasks.
type SharedCache = Arc<RwLock<LruCache<String, CachedResponse, RandomState>>>;

/// Global singleton instance of the in-memory cache.
/// Internally it uses an unbounded LRU (Least Recently Used) strategy and is guarded by a read-write lock.
/// Eviction is not time-based or size-based but rather triggered by system memory usage thresholds.
pub static MEMORY_CACHE: Lazy<SharedCache> = Lazy::new(|| {
    info!("🧠 Initializing unbounded LRU MEMORY_CACHE with dynamic memory-based eviction");
    Arc::new(RwLock::new(LruCache::unbounded_with_hasher(
        RandomState::default(),
    )))
});

/// Attempts to retrieve a response from the in-memory cache.
/// Returns `Some(CachedResponse)` if the key exists, otherwise `None`.
///
/// # Arguments
/// * `key` - A unique string key used to identify the cached response.
pub async fn get_from_memory(key: &str) -> Option<CachedResponse> {
    let mut cache = MEMORY_CACHE.write().await;
    cache.get(key).cloned()
}

/// Loads one or more entries into the in-memory cache and optionally triggers eviction if memory is constrained.
///
/// # Arguments
/// * `data` - A vector of (key, CachedResponse) pairs to be inserted into the cache.
pub async fn load_into_memory(data: Vec<(String, CachedResponse)>) {
    let mut cache = MEMORY_CACHE.write().await;

    for (k, v) in data {
        cache.put(k.clone(), v);
        
        info!("✅ Inserted key '{}' into MEMORY_CACHE", k);
    }

    maybe_evict_if_needed(&mut cache).await;
}

/// Monitors system memory usage and evicts LRU entries if usage exceeds the configured threshold.
/// This function is designed to prevent the application from consuming too much system memory.
///
/// The threshold is defined in `config.yaml` under `cache.memory_threshold`.
///
/// # Arguments
/// * `cache` - A mutable reference to the global LRU cache to perform eviction on.
pub async fn maybe_evict_if_needed(cache: &mut LruCache<String, CachedResponse, RandomState>) {
    let config = CONFIG.get();
    let threshold_percent = config
        .map(|c| c.cache.memory_threshold)
        .unwrap_or(80);

    let (used_kib, total_kib) = get_memory_usage_kib();
    let usage_percent = used_kib * 100 / total_kib;

    if usage_percent >= threshold_percent as u64 {
        
        info!(
            "⚠️ MEMORY_CACHE over threshold ({}% used). Cleaning LRU...",
            usage_percent
        );

        // Continue evicting entries until usage falls below threshold or the cache is empty
        while (get_memory_usage_kib().0 * 100 / total_kib) >= threshold_percent as u64 {
            if let Some((oldest_key, _)) = cache.pop_lru() {
                
                info!("🧹 Evicted key '{}' from MEMORY_CACHE", oldest_key);
            } else {
                
                break; // Nothing left to evict
            }
        }
    }
}

/// Retrieves the current system memory usage statistics from the operating system.
///
/// # Returns
/// A tuple representing the used and total memory in KiB (kibibytes).
/// * `(used_kib, total_kib)`
pub fn get_memory_usage_kib() -> (u64, u64) {
    let mut sys = System::new();
    sys.refresh_memory();

    let used = sys.used_memory(); // in KiB
    let total = sys.total_memory(); // in KiB

    (used, total)
}