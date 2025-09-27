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
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::{Client, config::Builder};
use bytes::Bytes;
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use once_cell::sync::OnceCell;
use serde_json;
use std::{error::Error, io::{Read, Write}};
use tracing::{error, info, warn};
use std::env; //MIA
use tokio::time::{sleep, Duration}; //MIA
use crate::proxy::CIRCUIT_BREAKER; // importar el breaker (pub(crate) en proxy.rs) MIA
use std::sync::atomic::Ordering; //MIA


/// Global instance of the AWS S3 client, initialized once and reused.
static S3_CLIENT: OnceCell<Client> = OnceCell::new();


/// Initializes the AWS S3 client from environment variables or default provider chain.
/// Region fallback is `us-east-1` if no environment setting is present.

pub async fn init_s3_client() {
    if S3_CLIENT.get().is_none() {
        let region_provider = RegionProviderChain::default_provider().or_else("us-east-1");
        let base_config = aws_config::from_env()
            .region(region_provider)
            .load()
            .await;

        // if AWS_ENDPOINT_URL exists → MinIO is used (or S3 compatible service)
        let client = if let Ok(endpoint) = env::var("AWS_ENDPOINT_URL") {
            let s3_config = Builder::from(&base_config)
                .endpoint_url(endpoint)
                .force_path_style(true) // Important for MinIO
                .build();
            Client::from_conf(s3_config)
        } else {
            Client::new(&base_config)
        };

        let _ = S3_CLIENT.set(client);

    }

    // Check the bucket in the config file
    let bucket = match CONFIG.get() {
        Some(cfg) if !cfg.s3_bucket.is_empty() => cfg.s3_bucket.clone(),
        _ => {
            tracing::error!("❌ s3_bucket not set in configuration");
            std::process::exit(1);
        }
    };
    
    // check connection to bucket
    if let Some(client) = S3_CLIENT.get() {
        if let Err(e) = client.head_bucket().bucket(&bucket).send().await {
            tracing::error!("❌ Error accessing bucket '{}': {:?}", bucket, e);
            std::process::exit(1);
        } else {
            tracing::info!("✅ Bucket '{}' Ok", bucket);
        }
    } else {
        // This shouldn't happen.
        tracing::error!("❌ S3 client not initialized");
        std::process::exit(1);
    
    }
}




/// Stores both response body and headers in AWS S3 using gzip compression.
///
/// - Body is stored under: `cache/{app_id}/{key}.gz`
/// - Headers are stored separately under: `cache/{app_id}/{key}.meta.gz`

pub async fn store_in_cache(
    key: String,
    data: Bytes,
    headers: Vec<(String, String)>
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let client = S3_CLIENT.get().ok_or("S3 client not initialized")?;
    let cfg = CONFIG.get().ok_or("CONFIG not initialized")?;
    let bucket = &cfg.s3_bucket;
    let app_id = &cfg.app_id;

    let data_path = format!("cache/{}/{}.gz", app_id, key);
    let meta_path = format!("cache/{}/{}.meta.gz", app_id, key);

    // Check if bucket is available
    client
        .head_bucket()
        .bucket(bucket)
        .send()
        .await
        .map_err(|e| {
            error!("❌ Error accessing bucket '{}': {:?}", bucket, e);
            Box::<dyn std::error::Error + Send + Sync>::from(e)
        })?;

    // Compress response body
    let compressed_data = {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&data).map_err(|e| {
            error!("⚠️ Failed to compress body for key '{}': {}", key, e);
            Box::<dyn std::error::Error + Send + Sync>::from(e)
        })?;
        encoder.finish().map_err(|e| {
            error!("⚠️ Failed to finish compression for key '{}': {}", key, e);
            Box::<dyn std::error::Error + Send + Sync>::from(e)
        })?
    };

    // Serialize and compress headers
    let compressed_meta = {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        let headers_json = serde_json::to_vec(&headers).map_err(|e| {
            error!("⚠️ Failed to serialize headers for key '{}': {}", key, e);
            Box::<dyn std::error::Error + Send + Sync>::from(e)
        })?;
        encoder.write_all(&headers_json).map_err(|e| {
            error!("⚠️ Failed to compress headers for key '{}': {}", key, e);
            Box::<dyn std::error::Error + Send + Sync>::from(e)
        })?;
        encoder.finish().map_err(|e| {
            error!("⚠️ Failed to finish compression for headers key '{}': {}", key, e);
            Box::<dyn std::error::Error + Send + Sync>::from(e)
        })?
    };

    // Upload compressed body to S3
    client
        .put_object()
        .bucket(bucket)
        .key(&data_path)
        .body(ByteStream::from(compressed_data))
        .content_type("application/gzip")
        .send()
        .await
        .map_err(|e| {
            error!("❌ Error uploading body for key '{}': {}", key, e);
            Box::<dyn std::error::Error + Send + Sync>::from(e)
        })?;

    // Upload compressed headers to S3
    client
        .put_object()
        .bucket(bucket)
        .key(&meta_path)
        .body(ByteStream::from(compressed_meta))
        .content_type("application/gzip")
        .send()
        .await
        .map_err(|e| {
            error!("❌ Error uploading headers for key '{}': {}", key, e);
            Box::<dyn std::error::Error + Send + Sync>::from(e)
        })?;

    info!("✅ Key '{}' stored in S3 bucket '{}'", key, bucket);
    Ok(())
}


/// Loads both body and headers from S3 and decompresses them.
/// If headers are missing or invalid, defaults to empty header list.

pub async fn load_from_cache(
    key: &str,
) -> Result<(Bytes, Vec<(String, String)>), Box<dyn Error + Send + Sync>> {
    let client = S3_CLIENT.get().ok_or("S3 client not initialized")?;
    let cfg = CONFIG.get().ok_or("CONFIG not initialized")?;
    let app_id = &cfg.app_id;
    let bucket = &cfg.s3_bucket;

    // Check if bucket is available
    client
        .head_bucket()
        .bucket(bucket)
        .send()
        .await
        .map_err(|e| {
            error!("❌ Error accessing bucket '{}': {:?}", bucket, e);
            Box::<dyn std::error::Error + Send + Sync>::from(e)
        })?;

    let data_path = format!("cache/{}/{}.gz", app_id, key);
    let meta_path = format!("cache/{}/{}.meta.gz", app_id, key);

    // Fetch and decompress body
    let resp = client
        .get_object()
        .bucket(bucket)
        .key(&data_path)
        .send()
        .await
        .map_err(|e| {
            warn!("❌ Object '{}' is not in the S3 cache: {}", key, e);
            Box::<dyn std::error::Error + Send + Sync>::from(format!(
                "Object '{}' is not in the S3 cache: {}", key, e
            ))
        })?;

    let collected = resp.body.collect().await.map_err(|e| {
        error!("⚠️ Failed to read body for key '{}': {}", key, e);
        Box::<dyn std::error::Error + Send + Sync>::from(format!("Failed to read body: {}", e))
    })?;

    let compressed = collected.into_bytes();
    let mut decoder = GzDecoder::new(&compressed[..]);
    let mut decompressed = Vec::new();

    decoder.read_to_end(&mut decompressed).map_err(|e| {
        error!("⚠️ Failed to decompress body for key '{}': {}", key, e);
        Box::<dyn std::error::Error + Send + Sync>::from(format!("Failed to decompress body: {}", e))
    })?;

    let data = Bytes::from(decompressed);

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
                    vec![]
                } else {
                    match serde_json::from_slice::<Vec<(String, String)>>(&decompressed) {
                        Ok(h) => h,
                        Err(e) => {
                            error!("⚠️ Failed to parse headers JSON for key '{}': {}", key, e);
                            vec![]
                        }
                    }
                }
            }
            Err(e) => {
                warn!("⚠️ Failed to read headers for key '{}': {}", key, e);
                vec![]
            }
        },
        Err(e) => {
            warn!("⚠️ Failed to get headers object '{}' from S3: {}", key, e);
            vec![]
        }
    };

    Ok((data, headers))
}

/// Deletes all cached objects (both `.gz` and `.meta.gz`) under `cache/{app_id}/` in the S3 bucket.
///
/// # Returns
/// - `Ok(count)` if all deletions succeeded or no files were found.
/// - `Err(_)` if any error occurred during listing or deletion.
pub async fn delete_all_from_cache() -> Result<usize, Box<dyn Error + Send + Sync>> {
    let client = S3_CLIENT
        .get()
        .ok_or_else(|| "S3 client not initialized".to_string())?;

    let config = CONFIG
        .get()
        .ok_or_else(|| "CONFIG not initialized".to_string())?;

    let prefix = format!("cache/{}/", config.app_id);
    let bucket = &config.s3_bucket;
    let mut continuation_token = None;
    let mut deleted_count = 0;

    loop {
        let resp = client
            .list_objects_v2()
            .bucket(bucket)
            .prefix(&prefix)
            .set_continuation_token(continuation_token.clone())
            .send()
            .await?;

        for obj in resp.contents() {
            if let Some(key) = obj.key() {
                match client.delete_object().bucket(bucket).key(key).send().await {
                    Ok(_) => {
                        info!("🗑️ Deleted S3 object '{}'", key);
                        deleted_count += 1;
                    }
                    Err(e) => {
                        warn!("⚠️ Failed to delete S3 object '{}': {}", key, e);
                    }
                }
            }
        }

        if resp.is_truncated() == Some(true) {
            continuation_token = resp.next_continuation_token().map(|s| s.to_string());
        } else {
            break;
        }
    }

    Ok(deleted_count)
}

/// Single connectivity check to the configured S3 bucket.
/// Logs result. Ok(()) = bucket accesible; Err(_) = fallo.
pub async fn check_bucket_connection() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client = S3_CLIENT.get().ok_or("S3 client not initialized")?;
    let cfg = CONFIG.get().ok_or("CONFIG not initialized")?;
    let bucket = &cfg.s3_bucket;

    match client.head_bucket().bucket(bucket).send().await {
        Ok(_) => {
            tracing::info!("✅ S3 health check OK (bucket='{}')", bucket);
            Ok(())
        }
        Err(e) => {
            tracing::warn!("⚠️ S3 health check FAILED (bucket='{}'): {:?}", bucket, e);
            Err(Box::<dyn std::error::Error + Send + Sync>::from(e))
        }
    }
}

/// Inicia un task que hace head_bucket() cada `interval_secs`.
/// Flujo esperado:
/// - Se llama cuando el circuito ya está en true (abierto).
/// - En cada fallo: mantiene CIRCUIT_BREAKER en true y reintenta.
/// - En el primer éxito: pone CIRCUIT_BREAKER = false y termina el task.
/// - interval_secs == 0 -> no arranca.
pub fn start_s3_health_checker(interval_secs: u64) {
    if interval_secs == 0 {
        tracing::info!("S3 health checker deshabilitado (intervalo = 0s)");
        return;
    }

    let dur = Duration::from_secs(interval_secs);
    tracing::info!(
        "🩺 Iniciando S3 health checker (intervalo {}s, breaker actual = {})",
        interval_secs,
        CIRCUIT_BREAKER.load(Ordering::SeqCst)
    );

    tokio::spawn(async move {
        loop {
            match check_bucket_connection().await {
                Ok(_) => {
                    tracing::info!(
                        "✅ S3 restaurado. Cerrando circuit breaker (true -> false) y deteniendo checker."
                    );
                    CIRCUIT_BREAKER.store(false, Ordering::SeqCst);
                    break;
                }
                Err(e) => {
                    // Mantener breaker abierto
                    CIRCUIT_BREAKER.store(true, Ordering::SeqCst);
                    tracing::warn!(
                        "⚠️ S3 aún inaccesible (breaker=true). Reintento en {}s. Error: {}",
                        interval_secs,
                        e
                    );
                }
            }
            sleep(dur).await;
        }
    });
}