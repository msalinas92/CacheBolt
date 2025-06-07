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

#[cfg(test)]
mod tests {
    use super::*;
    use cachebolt::{
        config::{Config, LatencyFailover, MaxLatencyRule, MemoryEviction, StorageBackend, CONFIG},
        memory::memory::{get_from_memory, get_memory_usage_kib, load_into_memory, maybe_evict_if_needed, CachedResponse, MEMORY_CACHE},
    };
    use bytes::Bytes;
    use ctor::ctor;

    #[ctor]
    fn init_tracing() {
        let _ = tracing_subscriber::fmt::try_init();
    }

    fn setup_config(threshold: usize) {
        let cfg = Config {
            app_id: "test".into(),
            gcs_bucket: "g".into(),
            s3_bucket: "s".into(),
            azure_container: "a".into(),
            max_concurrent_requests: 1,
            downstream_base_url: "http://localhost".into(),
            downstream_timeout_secs: 1,
            memory_eviction: MemoryEviction {
                threshold_percent: threshold,
            },
            latency_failover: LatencyFailover {
                default_max_latency_ms: 200,
                path_rules: vec![MaxLatencyRule {
                    pattern: "^/test".into(),
                    max_latency_ms: 100,
                }],
            },
            storage_backend: StorageBackend::Local,
        };

        // Set config only once
        let _ = CONFIG.set(cfg);
    }

    #[tokio::test]
    async fn test_cache_insertion_and_retrieval() {
        setup_config(90);
        let key = "test-key".to_string();
        let value = CachedResponse {
            body: Bytes::from("hello world"),
            headers: vec![("Content-Type".into(), "text/plain".into())],
        };

        load_into_memory(vec![(key.clone(), value.clone())]).await;
        let retrieved = get_from_memory(&key).await;

        assert!(retrieved.is_some(), "Cache should contain the key");
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.body, value.body);
        assert_eq!(retrieved.headers, value.headers);
    }

    #[tokio::test]
    async fn test_eviction_not_triggered_below_threshold() {
        setup_config(100); // High threshold to avoid eviction
        let key = "low-mem".to_string();
        let value = CachedResponse {
            body: Bytes::from("safe"),
            headers: vec![("x".into(), "y".into())],
        };

        load_into_memory(vec![(key.clone(), value)]).await;

        let mut cache = MEMORY_CACHE.write().await;
        let initial_len = cache.len();
        maybe_evict_if_needed(&mut cache).await;
        assert_eq!(cache.len(), initial_len);
    }

    #[tokio::test]
    async fn test_get_from_memory_none_if_not_found() {
        setup_config(90);
        let result = get_from_memory("non-existent").await;
        assert!(result.is_none(), "Should return None if key not found");
    }

    #[test]
    fn test_get_memory_usage_kib_has_values() {
        let (used, total) = get_memory_usage_kib();
        assert!(used > 0, "Used memory should be positive");
        assert!(total > 0, "Total memory should be positive");
        assert!(total >= used, "Total memory should be >= used memory");
    }

    #[tokio::test]
    async fn test_bulk_load_into_memory() {
        setup_config(95);

        let entries = vec![
            (
                "key-1".to_string(),
                CachedResponse {
                    body: Bytes::from("value-1"),
                    headers: vec![("a".into(), "1".into())],
                },
            ),
            (
                "key-2".to_string(),
                CachedResponse {
                    body: Bytes::from("value-2"),
                    headers: vec![("b".into(), "2".into())],
                },
            ),
        ];

        load_into_memory(entries.clone()).await;

        for (key, original) in entries {
            let cached = get_from_memory(&key).await;
            assert!(cached.is_some(), "Expected key '{}' to exist", key);

            let cached = cached.unwrap();
            assert_eq!(
                cached.body, original.body,
                "Body mismatch for key '{}'",
                key
            );
            assert_eq!(
                cached.headers, original.headers,
                "Headers mismatch for key '{}'",
                key
            );
        }
    }
}
