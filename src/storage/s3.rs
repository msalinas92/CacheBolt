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
use aws_config::meta::region::RegionProviderChain;
use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;
use bytes::Bytes;
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use once_cell::sync::OnceCell;
use serde_json;
use std::io::{Read, Write};
use tracing::{error, info, warn};

/// Global instance of the AWS S3 client, initialized once and reused.
static S3_CLIENT: OnceCell<Client> = OnceCell::new();

/// Initializes the AWS S3 client from environment variables or default provider chain.
/// Region fallback is `us-east-1` if no environment setting is present.
#[cfg(not(tarpaulin_include))]
pub async fn init_s3_client() {
    if S3_CLIENT.get().is_none() {
        let region_provider = RegionProviderChain::default_provider().or_else("us-east-1");
        let config = aws_config::from_env().region(region_provider).load().await;
        let client = Client::new(&config);
        let _ = S3_CLIENT.set(client);
    }
}

/// Stores both response body and headers in AWS S3 using gzip compression.
///
/// - Body is stored under: `cache/{app_id}/{key}.gz`
/// - Headers are stored separately under: `cache/{app_id}/{key}.meta.gz`
#[cfg(not(tarpaulin_include))]
pub async fn store_in_cache(key: String, data: Bytes, headers: Vec<(String, String)>) {
    let client = match S3_CLIENT.get() {
        Some(c) => c,
        None => {
            error!("S3 client not initialized");
            return;
        }
    };

    let bucket = match CONFIG.get() {
        Some(cfg) => cfg.s3_bucket.clone(),
        None => {
            error!("CONFIG not initialized; cannot read s3_bucket");
            return;
        }
    };

    let app_id = CONFIG
        .get()
        .map(|c| c.app_id.clone())
        .unwrap_or_else(|| "default".into());
    let data_path = format!("cache/{}/{}.gz", app_id, key);
    let meta_path = format!("cache/{}/{}.meta.gz", app_id, key);

    // Compress response body
    let compressed_data = {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        if encoder.write_all(&data).is_err() {
            error!("Error compressing body for key '{}'", key);
            return;
        }
        match encoder.finish() {
            Ok(c) => c,
            Err(e) => {
                error!("Error finalizing compression for key '{}': {}", key, e);
                return;
            }
        }
    };

    // Serialize and compress headers
    let compressed_meta = {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        let headers_json = match serde_json::to_vec(&headers) {
            Ok(json) => json,
            Err(e) => {
                error!("Error serializing headers for '{}': {}", key, e);
                return;
            }
        };
        if encoder.write_all(&headers_json).is_err() {
            error!("Error compressing headers for key '{}'", key);
            return;
        }
        match encoder.finish() {
            Ok(c) => c,
            Err(e) => {
                error!(
                    "Error finalizing header compression for key '{}': {}",
                    key, e
                );
                return;
            }
        }
    };

    // Upload compressed body to S3
    let _ = client
        .put_object()
        .bucket(&bucket)
        .key(&data_path)
        .body(ByteStream::from(compressed_data))
        .content_type("application/gzip")
        .send()
        .await;

    // Upload compressed headers to S3
    let _ = client
        .put_object()
        .bucket(&bucket)
        .key(&meta_path)
        .body(ByteStream::from(compressed_meta))
        .content_type("application/gzip")
        .send()
        .await;

    info!("✅ Key '{}' stored in S3 bucket '{}'", key, bucket);
}

/// Loads both body and headers from S3 and decompresses them.
/// If headers are missing or invalid, defaults to empty header list.
#[cfg(not(tarpaulin_include))]
pub async fn load_from_cache(key: &str) -> Option<(Bytes, Vec<(String, String)>)> {
    let client = S3_CLIENT.get()?;
    let cfg = CONFIG.get()?;
    let app_id = &cfg.app_id;
    let bucket = &cfg.s3_bucket;

    let data_path = format!("cache/{}/{}.gz", app_id, key);
    let meta_path = format!("cache/{}/{}.meta.gz", app_id, key);

    // Fetch and decompress body
    let data = match client
        .get_object()
        .bucket(bucket)
        .key(&data_path)
        .send()
        .await
    {
        Ok(resp) => match resp.body.collect().await {
            Ok(collected) => {
                let compressed = collected.into_bytes();
                let mut decoder = GzDecoder::new(&compressed[..]);
                let mut decompressed = Vec::new();
                if decoder.read_to_end(&mut decompressed).is_err() {
                    error!("⚠️ Failed to decompress body for key '{}'", key);
                    return None;
                }
                Bytes::from(decompressed)
            }
            Err(e) => {
                error!("⚠️ Failed to read body for key '{}': {}", key, e);
                return None;
            }
        },
        Err(e) => {
            warn!("❌ Failed to get object '{}' from S3: {}", key, e);
            return None;
        }
    };

    // Fetch and decompress headers (optional fallback to empty)
    let headers = match client
        .get_object()
        .bucket(bucket)
        .key(&meta_path)
        .send()
        .await
    {
        Ok(resp) => match resp.body.collect().await {
            Ok(collected) => {
                let compressed = collected.into_bytes();
                let mut decoder = GzDecoder::new(&compressed[..]);
                let mut decompressed = Vec::new();
                if decoder.read_to_end(&mut decompressed).is_err() {
                    error!("⚠️ Failed to decompress headers for key '{}'", key);
                    return Some((data, vec![]));
                }
                match serde_json::from_slice::<Vec<(String, String)>>(&decompressed) {
                    Ok(h) => h,
                    Err(e) => {
                        error!("⚠️ Failed to parse headers JSON for key '{}': {}", key, e);
                        vec![]
                    }
                }
            }
            Err(e) => {
                warn!("⚠️ Failed to read headers for key '{}': {}", key, e);
                vec![]
            }
        },
        Err(_) => vec![], // If headers object is missing, default to empty
    };

    Some((data, headers))
}
