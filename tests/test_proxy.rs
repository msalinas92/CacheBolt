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
    use axum::response::IntoResponse;
    use bytes::Bytes;
    use cachebolt::{
        config::{CONFIG, StorageBackend},
        proxy::{
            MAX_CONCURRENT_REQUESTS, SEMAPHORE, build_response, forward_request, hash_uri,
            proxy_handler, try_cache,
        },
    };
    use hyper::{Body, Client, Request, Response, body::to_bytes};
    use std::sync::Arc;
    use tokio::sync::{Semaphore, mpsc};

    #[tokio::test]
    async fn test_hash_uri_consistency() {
        let uri = "/api/test";
        let hash1 = hash_uri(uri);
        let hash2 = hash_uri(uri);
        assert_eq!(
            hash1, hash2,
            "Hashes should be consistent for the same input"
        );
    }

    #[tokio::test]
    async fn test_build_response_with_headers() {
        let body = Bytes::from_static(b"hello world");
        let headers = vec![
            ("content-type".to_string(), "text/plain".to_string()),
            ("x-custom".to_string(), "123".to_string()),
        ];

        let response = build_response(body.clone(), headers.clone());
        let (parts, body_out) = response.into_parts();
        let body_bytes = to_bytes(body_out).await.unwrap();

        assert_eq!(body_bytes, body);
        assert_eq!(parts.headers.get("content-type").unwrap(), "text/plain");
        assert_eq!(parts.headers.get("x-custom").unwrap(), "123");
    }

    #[tokio::test]
    async fn test_build_response_sets_default_content_type() {
        let body = Bytes::from_static(b"no content type");
        let headers = vec![("x-something".to_string(), "value".to_string())];

        let response = build_response(body.clone(), headers.clone());
        let content_type = response.headers().get("content-type").unwrap();
        assert_eq!(content_type, "application/octet-stream");
    }

    #[tokio::test]
    async fn test_try_cache_returns_502_when_empty() {
        // try_cache ahora devuelve Result<Response, Error>, obtener el Response primero
        let resp = try_cache("nonexistent-key")
            .await
            .expect("try_cache returned an error");
        assert_eq!(resp.status().as_u16(), 502);
    }

    #[tokio::test]
    async fn test_concurrency_semaphore_limit_blocks() {
        let original_limit = *MAX_CONCURRENT_REQUESTS;
        let semaphore = Arc::new(Semaphore::new(1));

        let permit1 = semaphore
            .clone()
            .try_acquire_owned()
            .expect("first should succeed");
        let permit2 = semaphore.clone().try_acquire_owned();

        assert!(permit2.is_err(), "second should fail due to limit");
        drop(permit1);
    }

    #[tokio::test]
    async fn test_hash_uri_differs_for_different_input() {
        let a = hash_uri("/a");
        let b = hash_uri("/b");
        assert_ne!(a, b);
    }

    #[tokio::test]
    async fn test_forward_request_fails_without_server() {
        let _ = CONFIG.set(cachebolt::config::Config {
            app_id: "x".into(),
            gcs_bucket: "".into(),
            s3_bucket: "".into(),
            azure_container: "".into(),
            max_concurrent_requests: 1,
            downstream_base_url: "http://127.0.0.1:9999".into(),
            cache: cachebolt::config::CacheSettings {
                memory_threshold: 90,
                refresh_percentage: 10,
                ttl_seconds: 300,
            },
            latency_failover: cachebolt::config::LatencyFailover {
                default_max_latency_ms: 1000,
                path_rules: vec![],
            },
            storage_backend: StorageBackend::Local,
            storage_backend_failures: 0,
            backend_retry_interval_secs: 0,
            ignored_headers: None,
            proxy_port: 3000,
            admin_port: 3001
        });

        let dummy_request = Request::builder()
            .method("GET")
            .uri("/notfound")
            .body(Body::empty())
            .unwrap();

        let result = forward_request("/notfound", dummy_request).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_semaphore_enforces_limit() {
        // Intenta adquirir más permisos de los permitidos
        let permits = *MAX_CONCURRENT_REQUESTS + 1;
        let mut acquired = Vec::new();

        for _ in 0..*MAX_CONCURRENT_REQUESTS {
            let permit = SEMAPHORE.clone().try_acquire_owned();
            assert!(permit.is_ok(), "Should acquire permit within limit");
            acquired.push(permit);
        }

        let extra = SEMAPHORE.clone().try_acquire_owned();
        assert!(extra.is_err(), "Should fail when exceeding permit limit");

        // Libera uno y prueba que ahora sí se puede adquirir uno más
        drop(acquired.pop());
        let retry = SEMAPHORE.clone().try_acquire_owned();
        assert!(retry.is_ok(), "Should acquire permit after releasing one");
    }

    #[tokio::test]
    async fn test_proxy_handler_downstream_fail_no_cache() {
        let _ = CONFIG.set(cachebolt::config::Config {
            app_id: "x".into(),
            gcs_bucket: "".into(),
            s3_bucket: "".into(),
            azure_container: "".into(),
            max_concurrent_requests: 1,
            downstream_base_url: "http://127.0.0.1:9999".into(), // puerto inválido
            cache: cachebolt::config::CacheSettings {
                memory_threshold: 90,
                refresh_percentage: 10,
                ttl_seconds: 300,
            },
            latency_failover: cachebolt::config::LatencyFailover {
                default_max_latency_ms: 1000,
                path_rules: vec![],
            },
            storage_backend: StorageBackend::Local,
            storage_backend_failures: 0,
            backend_retry_interval_secs: 0,
            ignored_headers: None,
            proxy_port: 3000,
            admin_port: 3001
        });

        let req = Request::builder()
            .method("GET")
            .uri("/fail")
            .body(Body::empty())
            .unwrap();

        let resp = proxy_handler(req).await.into_response();
        assert_eq!(resp.status(), 502);
    }

    #[tokio::test]
    async fn test_proxy_handler_concurrency_full_and_no_cache() {
        let _ = CONFIG.set(cachebolt::config::Config {
            app_id: "x".into(),
            gcs_bucket: "".into(),
            s3_bucket: "".into(),
            azure_container: "".into(),
            max_concurrent_requests: 1,
            downstream_base_url: "http://127.0.0.1:9999".into(),
            cache: cachebolt::config::CacheSettings {
                memory_threshold: 90,
                refresh_percentage: 10,
                ttl_seconds: 300,
            },
            latency_failover: cachebolt::config::LatencyFailover {
                default_max_latency_ms: 1000,
                path_rules: vec![],
            },
            storage_backend: StorageBackend::Local,
            storage_backend_failures: 0,
            backend_retry_interval_secs: 0,
            ignored_headers: None,
            proxy_port: 3000,
            admin_port: 3001
        });

        // Saturar manualmente
        let _permit = SEMAPHORE
            .clone()
            .try_acquire_owned()
            .expect("should acquire");

        let req = Request::builder()
            .method("GET")
            .uri("/uncached")
            .body(Body::empty())
            .unwrap();

        let resp = proxy_handler(req).await.into_response();
        assert_eq!(resp.status(), 502);

        let body_bytes = hyper::body::to_bytes(resp.into_body()).await.unwrap();
        let body_str = String::from_utf8_lossy(&body_bytes);

        assert!(
            body_str.contains("Too many concurrent requests")
                || body_str.contains("Downstream error and no cache"),
            "Expected fallback 502 message, got: {}",
            body_str
        );
    }
}
