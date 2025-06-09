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
use axum::response::IntoResponse;
use bytes::Bytes;
use hyper::client::HttpConnector;
use hyper::{Body, Client, Request, Response};
use once_cell::sync::Lazy;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::sync::{Semaphore, mpsc};
use tokio::time::{Duration, Instant, timeout};

use crate::config::{CONFIG, StorageBackend};
use crate::memory::memory;
use crate::rules::latency::{get_max_latency_for_path, mark_latency_fail, should_failover};
use crate::storage::{azure, gcs, local, s3};

// ------------------------------------------
// GLOBAL SHARED STATE
// ------------------------------------------

/// Maximum concurrent downstream requests allowed
pub static MAX_CONCURRENT_REQUESTS: Lazy<usize> = Lazy::new(|| {
    CONFIG
        .get()
        .map(|c| c.max_concurrent_requests)
        .unwrap_or(200)
});

/// Semaphore to enforce concurrency limits on outgoing requests
pub static SEMAPHORE: Lazy<Arc<Semaphore>> =
    Lazy::new(|| Arc::new(Semaphore::new(*MAX_CONCURRENT_REQUESTS)));

/// Shared HTTP client for all outbound requests
static HTTP_CLIENT: Lazy<Client<HttpConnector>> = Lazy::new(Client::new);

/// Background task that persistently writes cache entries to the configured backend

static CACHE_WRITER: Lazy<mpsc::Sender<(String, Bytes, Vec<(String, String)>)>> = Lazy::new(|| {
    let (tx, mut rx) = mpsc::channel::<(String, Bytes, Vec<(String, String)>)>(100);
    tokio::spawn(async move {
        while let Some((key, data, headers)) = rx.recv().await {
            match CONFIG.get().map(|c| &c.storage_backend) {
                Some(StorageBackend::Azure) => azure::store_in_cache(key, data, headers).await,
                Some(StorageBackend::Gcs) => gcs::store_in_cache(key, data, headers).await,
                Some(StorageBackend::Local) => local::store_in_cache(key, data, headers).await,
                Some(StorageBackend::S3) => s3::store_in_cache(key, data, headers).await,
                None => {
                    tracing::error!("❌ CONFIG not initialized. Unable to persist cache.");
                }
            }
        }
    });
    tx
});

/// Main proxy handler that receives incoming requests and delegates to downstream or cache
pub async fn proxy_handler(req: Request<Body>) -> impl IntoResponse {
    let uri = req.uri().to_string();
    // Fetch ignored headers set from config (lowercased for comparison)
    let ignored = CONFIG
        .get()
        .map(|c| c.ignored_headers_set().clone())
        .unwrap_or_default();

    // Extract and normalize headers, excluding those in the ignored set
    let mut headers_kv = req
        .headers()
        .iter()
        .filter(|(k, _)| {
            let key_lower = k.as_str().to_ascii_lowercase();
            !ignored.contains(&key_lower)
        })
        .map(|(k, v)| {
            (
                k.as_str().to_ascii_lowercase(),      // normalize key
                v.to_str().unwrap_or("").to_string(), // safe string conversion
            )
        })
        .collect::<Vec<_>>();

    // Sort headers alphabetically to ensure deterministic key
    headers_kv.sort_by(|a, b| a.0.cmp(&b.0));

    // Join headers as "key:value" pairs separated by semicolons
    let relevant_headers = headers_kv
        .iter()
        .map(|(k, v)| format!("{}:{}", k, v))
        .collect::<Vec<_>>()
        .join(";");

    // Compose cache key from URI and relevant headers
    let key_source = format!("{}|{}", uri, relevant_headers);
    let key = hash_uri(&key_source);

    // If the URI is in failover mode, serve from cache
    if should_failover(&uri) {
        tracing::info!("⚠️ Using fallback due to recent high latency for '{}'", uri);
        return try_cache(&key).await;
    }

    // Try memory cache first
    if let Some(cached) = memory::get_from_memory(&key).await {
        return build_response(cached.body.clone(), cached.headers.clone());
    }

    // Try to acquire concurrency slot
    match SEMAPHORE.clone().try_acquire_owned() {
        Ok(_permit) => {
            let start = Instant::now();

            // Reconstruct request from parts (to forward it with headers)
            let (parts, body) = req.into_parts();
            let req = Request::from_parts(parts, body);

            match forward_request(&uri, req).await {
                Ok(resp) => {
                    let elapsed_ms = start.elapsed().as_millis() as u64;
                    let threshold_ms = get_max_latency_for_path(&uri);

                    if elapsed_ms > threshold_ms {
                        tracing::warn!(
                            "🚨 Latency {}ms exceeded threshold {}ms for '{}'",
                            elapsed_ms,
                            threshold_ms,
                            uri
                        );
                        mark_latency_fail(&uri);
                    }

                    // Split response into parts
                    let (parts, body) = resp.into_parts();
                    let body_bytes = hyper::body::to_bytes(body).await.unwrap_or_default();

                    let headers_vec = parts
                        .headers
                        .iter()
                        .map(|(k, v)| {
                            (k.as_str().to_string(), v.to_str().unwrap_or("").to_string())
                        })
                        .collect::<Vec<_>>();

                    // Cache response in memory and send to backend storage
                    let cached_response = memory::CachedResponse {
                        body: body_bytes.clone(),
                        headers: headers_vec.clone(),
                    };

                    if !should_failover(&uri) {
                        memory::load_into_memory(vec![(key.clone(), cached_response)]).await;
                        let _ = CACHE_WRITER
                            .send((key.clone(), body_bytes.clone(), headers_vec))
                            .await;
                    } else {
                        tracing::info!(
                            "🚫 Skipping cache store due to fallback mode for '{}'",
                            uri
                        );
                    }

                    Response::from_parts(parts, Body::from(body_bytes))
                }
                Err(_) => {
                    tracing::warn!("⛔ Downstream service failed for '{}'", uri);
                    try_cache(&key).await
                }
            }
        }
        Err(_) => {
            // If over concurrency limit, fallback to cache if possible
            if let Some(cached) = memory::get_from_memory(&key).await {
                build_response(cached.body.clone(), cached.headers.clone())
            } else {
                Response::builder()
                    .status(502)
                    .body("Too many concurrent requests and no cache available".into())
                    .unwrap()
            }
        }
    }
}

/// Attempts to retrieve response from memory or persistent cache
pub async fn try_cache(key: &str) -> Response<Body> {
    // Try memory first
    if let Some(cached) = memory::get_from_memory(key).await {
        tracing::info!("✅ Fallback hit from MEMORY_CACHE for '{}'", key);
        return build_response(cached.body.clone(), cached.headers.clone());
    }

    // Then check persistent cache backend
    let fallback = match CONFIG.get().map(|c| &c.storage_backend) {
        Some(StorageBackend::Azure) => azure::load_from_cache(key).await,
        Some(StorageBackend::Gcs) => gcs::load_from_cache(key).await,
        Some(StorageBackend::Local) => local::load_from_cache(key).await,
        Some(StorageBackend::S3) => s3::load_from_cache(key).await,
        None => None,
    };

    if let Some((data, headers)) = fallback {
        tracing::info!("✅ Fallback from persistent cache for '{}'", key);
        let cached_response = memory::CachedResponse {
            body: data.clone(),
            headers: headers.clone(),
        };
        memory::load_into_memory(vec![(key.to_string(), cached_response)]).await;
        build_response(data, headers)
    } else {
        Response::builder()
            .status(502)
            .body("Downstream error and no cache".into())
            .unwrap()
    }
}

/// Composes a full HTTP response from body and headers
pub fn build_response(body: Bytes, headers: Vec<(String, String)>) -> Response<Body> {
    let mut builder = Response::builder();
    let mut has_content_type = false;

    for (name, value) in headers.iter() {
        if name.eq_ignore_ascii_case("content-type") {
            has_content_type = true;
        }
        builder = builder.header(name, value);
    }

    if !has_content_type {
        builder = builder.header("Content-Type", "application/octet-stream");
    }

    builder.body(Body::from(body)).unwrap()
}

/// Returns a SHA256 hash string from a URI + headers
pub fn hash_uri(uri: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(uri.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Sends an outbound GET request to the downstream backend
pub async fn forward_request(uri: &str, original_req: Request<Body>) -> Result<Response<Body>, ()> {
    let cfg = CONFIG.get().unwrap();
    let full_url = format!("{}{}", cfg.downstream_base_url, uri);

    let mut builder = Request::builder().uri(full_url.clone()).method("GET");

    // Clona los headers del request original
    for (key, value) in original_req.headers().iter() {
        builder = builder.header(key, value);
    }

    // Construye el nuevo request
    let req = match builder.body(Body::empty()) {
        Ok(req) => req,
        Err(e) => {
            tracing::error!("❌ Error building downstream request: {}", e);
            return Err(());
        }
    };

    match timeout(
        Duration::from_secs(cfg.downstream_timeout_secs),
        HTTP_CLIENT.request(req),
    )
    .await
    {
        Ok(Ok(resp)) => Ok(resp),
        Ok(Err(e)) => {
            tracing::warn!("❌ Request to downstream '{}' failed: {}", full_url, e);
            Err(())
        }
        Err(_) => {
            tracing::warn!(
                "⏱ Timeout after {}s for '{}'",
                cfg.downstream_timeout_secs,
                full_url
            );
            Err(())
        }
    }
}
