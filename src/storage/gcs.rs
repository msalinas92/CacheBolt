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


// GCS client and request types from the google-cloud-storage crate
use google_cloud_storage::{
    client::Client,
    http::objects::{
        download::Range,
        get::GetObjectRequest,
        upload::{Media, UploadObjectRequest, UploadType},
    },
};
use bytes::Bytes;
use std::{borrow::Cow};
use std::sync::OnceLock;
use flate2::write::GzEncoder;
use flate2::read::GzDecoder;
use flate2::Compression;
use std::io::{Read, Write};
use tracing::{info, error, warn};
use crate::config::CONFIG;
use serde::{Serialize, Deserialize};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;

/// Global singleton GCS client instance, initialized at runtime.
pub static GCS_CLIENT: OnceLock<Client> = OnceLock::new();

/// Serializable structure to store cached response data in GCS.
/// - `body`: Base64-encoded response body.
/// - `headers`: Associated response headers.
#[derive(Serialize, Deserialize)]
struct CachedBlob {
    body: String,
    headers: Vec<(String, String)>,
}

/// Uploads a new cached object into GCS using the `cache/{app_id}/{key}` path.
/// The body is base64-encoded, then compressed with Gzip before being stored.
///
/// # Arguments
/// - `key`: Unique identifier for the object.
/// - `data`: Raw body bytes to be cached.
/// - `headers`: Response headers to store alongside the body.

pub async fn store_in_cache(key: String, data: Bytes, headers: Vec<(String, String)>) {
    // Retrieve initialized GCS client
    let client = match GCS_CLIENT.get() {
        Some(c) => c,
        None => {
            error!("GCS client is not initialized");
            return;
        }
    };

    // Load bucket name from config
    let bucket = match CONFIG.get() {
        Some(cfg) => cfg.gcs_bucket.clone(),
        None => {
            error!("CONFIG is not initialized; cannot get GCS bucket");
            return;
        }
    };

    // Build a serializable blob (body + headers) using base64 encoding
    let blob = CachedBlob {
        body: STANDARD.encode(&data),
        headers,
    };

    // Serialize the struct into JSON
    let json_bytes = match serde_json::to_vec(&blob) {
        Ok(v) => v,
        Err(e) => {
            error!("Failed to serialize JSON for key '{key}': {e}");
            return;
        }
    };

    // Compress the JSON using Gzip
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    if let Err(e) = encoder.write_all(&json_bytes) {
        error!("Failed to compress data for key '{key}': {e}");
        return;
    }

    let compressed = match encoder.finish() {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to finalize compression for key '{key}': {e}");
            return;
        }
    };

    // Build storage path: cache/{app_id}/{key}
    let app_id = &CONFIG.get().map(|c| c.app_id.clone()).unwrap_or_else(|| "default".into());
    let path = format!("cache/{app_id}/{}", key);

    // Build GCS upload request
    let req = UploadObjectRequest {
        bucket: bucket.clone(),
        ..Default::default()
    };

    let media = Media {
        name: Cow::Owned(path.clone()),
        content_type: Cow::Borrowed("application/gzip"),
        content_length: Some(compressed.len() as u64),
    };

    // Perform the upload using GCS simple upload API
    if let Err(e) = client.upload_object(&req, compressed, &UploadType::Simple(media)).await {
        error!("Failed to upload to GCS: bucket='{bucket}', object='{path}': {e}");
    } else {
        
        info!("✅ Stored key '{key}' in GCS bucket '{bucket}'");
    }
}

/// Loads and decompresses a cached object from GCS using the key.
///
/// # Arguments
/// - `key`: The object key within the cache path.
///
/// # Returns
/// - `Some((body, headers))` on success
/// - `None` if retrieval, decompression, or deserialization fails

pub async fn load_from_cache(key: &str) -> Option<(Bytes, Vec<(String, String)>)> {
    let client = GCS_CLIENT.get()?; // Get the global GCS client
    let bucket = CONFIG.get()?.gcs_bucket.clone(); // Load bucket from config
    let app_id = CONFIG.get().map(|c| c.app_id.clone()).unwrap_or_else(|| "default".into());
    let path = format!("cache/{app_id}/{}", key); // Construct full object path

    // Prepare the request to get the object
    let req = GetObjectRequest {
        bucket: bucket.clone(),
        object: path.clone(),
        ..Default::default()
    };

    // Attempt to download the Gzipped object from GCS
    match client.download_object(&req, &Range::default()).await {
        Ok(compressed) => {
            let mut decoder = GzDecoder::new(&*compressed); // Create gzip reader
            let mut decompressed = Vec::new();
            if decoder.read_to_end(&mut decompressed).is_err() {
                error!("Failed to decompress object '{path}' from bucket '{bucket}'");
                return None;
            }

            // Deserialize JSON into CachedBlob struct
            match serde_json::from_slice::<CachedBlob>(&decompressed) {
                Ok(blob) => {
                    // Decode base64-encoded body
                    match STANDARD.decode(&blob.body) {
                        Ok(body) => Some((Bytes::from(body), blob.headers)),
                        Err(e) => {
                            error!("Failed to decode base64 for key '{key}': {e}");
                            None
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to parse JSON for key '{key}': {e}");
                    None
                }
            }
        }
        Err(e) => {
            warn!("Failed to download object '{path}' from bucket '{bucket}': {e}");
            None
        }
    }
}
