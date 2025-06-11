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
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bytes::Bytes;
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use serde::{Deserialize, Serialize};
use std::{
    error::Error, fs::{self, File}, io::{Read, Write}, path::PathBuf
};
use tracing::{error, info, warn};
use std::fs::read_dir;

/// Struct representing a cached response.
/// - `body`: Base64-encoded body bytes.
/// - `headers`: Response headers as key-value pairs.
#[derive(Serialize, Deserialize)]
pub struct CachedBlob {
    pub body: String,
    pub headers: Vec<(String, String)>,
}

/// Constructs the full filesystem path for a given cache key.
/// Format: `storage/cache/{app_id}/{key}.gz`
pub fn build_local_cache_path(key: &str) -> Option<PathBuf> {
    let config = CONFIG.get()?;
    let app_id = &config.app_id;

    let mut path = PathBuf::from("storage/cache");
    path.push(app_id);
    path.push(format!("{key}.gz"));

    Some(path)
}

/// Stores a base64+Gzip-encoded blob (body + headers) to local disk.
/// Creates intermediate directories if needed.
///
/// # Arguments
/// - `key`: Cache key used as filename.
/// - `data`: Raw body bytes.
/// - `headers`: HTTP headers to store.
pub async fn store_in_cache(key: String, data: Bytes, headers: Vec<(String, String)>) {
    let path = match build_local_cache_path(&key) {
        Some(p) => p,
        None => {
            
            error!("CONFIG is not initialized; cannot build cache path");
            return;
        }
    };

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            
            error!(
                "Failed to create local storage directory {:?}: {}",
                parent, e
            );
            return;
        }
    }

    // Construct the CachedBlob struct to serialize
    let blob = CachedBlob {
        body: STANDARD.encode(&data),
        headers,
    };

    // Serialize to JSON
    let json = match serde_json::to_vec(&blob) {
        Ok(j) => j,
        
        Err(e) => {
            
            error!("Failed to serialize blob for '{}': {}", key, e);
            return;
        }
    };

    // Compress the JSON using gzip
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    if let Err(e) = encoder.write_all(&json) {
        
        error!("Failed to compress data for key '{}': {}", key, e);
        return;
    }

    
    let compressed = match encoder.finish() {
        Ok(c) => c,
        
        Err(e) => {
            
            error!("Failed to finalize compression for key '{}': {}", key, e);
            return;
        }
    };

    // Write compressed data to file
    match File::create(&path) {
        Ok(mut file) => {
            if let Err(e) = file.write_all(&compressed) {
                
                error!("Failed to write compressed file for key '{}': {}", key, e);
            } else {
                
                info!("✅ Stored key '{}' in local cache at {:?}", key, path);
            }
        }
        Err(e) => {
            
            error!("Failed to create file for key '{}': {}", key, e);
        }
    }
}

/// Loads a previously cached blob from local filesystem, decompresses and decodes it.
///
/// # Arguments
/// - `key`: Cache key corresponding to filename.
///
/// # Returns
/// - Some((body_bytes, headers)) on success.
/// - None on error or file not found.
pub async fn load_from_cache(key: &str) -> Option<(Bytes, Vec<(String, String)>)> {
    let path = build_local_cache_path(key)?;

    // Read compressed file from disk
    let compressed = match fs::read(&path) {
        Ok(data) => data,
        Err(e) => {
            warn!("Failed to read cached file {:?}: {}", path, e);
            return None;
        }
    };

    // Decompress using gzip
    let mut decoder = GzDecoder::new(&compressed[..]);
    let mut decompressed = Vec::new();
    if let Err(e) = decoder.read_to_end(&mut decompressed) {
        error!("Failed to decompress local cache file {:?}: {}", path, e);
        return None;
    }

    // Parse JSON blob and decode body
    match serde_json::from_slice::<CachedBlob>(&decompressed) {
        Ok(blob) => match STANDARD.decode(&blob.body) {
            Ok(decoded) => Some((Bytes::from(decoded), blob.headers)),
            Err(e) => {
                error!("Failed to decode base64 body for key '{}': {}", key, e);
                None
            }
        },
        Err(e) => {
            error!("Failed to parse cached JSON for key '{}': {}", key, e);
            None
        }
    }
}

/// Deletes all cached files for the current `app_id` from local storage.
///
/// # Returns
/// - `true` if all matching files were deleted or folder was empty.
/// - `false` if some deletions failed or CONFIG was not initialized.
/// Deletes all cached files for the current `app_id` from local storage.
///
/// # Returns
/// - `Ok(count)` with number of files deleted.
/// - `Err(...)` if reading or deleting fails.
pub async fn delete_all_from_cache() -> Result<usize, Box<dyn Error + Send + Sync>> {
    let config = CONFIG
        .get()
        .ok_or("CONFIG is not initialized; cannot delete local cache")?;

    let dir_path = PathBuf::from(format!("storage/cache/{}", config.app_id));
    let mut deleted = 0;

    match read_dir(&dir_path) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("gz") {
                    match fs::remove_file(&path) {
                        Ok(_) => {
                            deleted += 1;
                            info!("🗑️ Deleted local cache file {:?}", path);
                        }
                        Err(e) => {
                            warn!("⚠️ Failed to delete file {:?}: {}", path, e);
                        }
                    }
                }
            }

            info!("✅ Deleted {deleted} local cache files under {:?}", dir_path);
            Ok(deleted)
        }
        Err(e) => {
            Err(format!("Failed to read local cache directory: {e}").into())
        }
    }
}