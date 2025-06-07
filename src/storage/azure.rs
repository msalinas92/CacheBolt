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

// Azure SDK dependencies for Blob storage access
use azure_storage::StorageCredentials;
use azure_storage_blobs::prelude::*;
use bytes::Bytes;
use once_cell::sync::OnceCell;
use std::env;
use tracing::{error, info, warn};

use crate::config::CONFIG;

use serde::{Serialize, Deserialize};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;

/// Structure used to store a cached object in Azure Blob Storage.
/// - `body`: base64-encoded content (response body).
/// - `headers`: original response headers.
#[derive(Serialize, Deserialize)]
struct CachedBlob {
    body: String,
    headers: Vec<(String, String)>,
}

/// Global singleton instance of the Azure Blob client.
/// It is lazily initialized and shared across all tasks.
static AZURE_CLIENT: OnceCell<BlobServiceClient> = OnceCell::new();

/// Initializes the Azure Blob Storage client based on environment variables:
/// - `AZURE_STORAGE_ACCOUNT`
/// - `AZURE_STORAGE_ACCESS_KEY`
///
/// This function should be called only once at startup.
pub fn init_azure_client() {
    if AZURE_CLIENT.get().is_none() {
        // Retrieve Azure credentials from environment variables
        let account = env::var("AZURE_STORAGE_ACCOUNT")
            .expect("Missing environment variable AZURE_STORAGE_ACCOUNT");
        let access_key = env::var("AZURE_STORAGE_ACCESS_KEY")
            .expect("Missing environment variable AZURE_STORAGE_ACCESS_KEY");

        // Construct credentials and instantiate the Azure client
        let credentials = StorageCredentials::access_key(account.clone(), access_key);
        let client = BlobServiceClient::new(account, credentials);

        // Store client in the OnceCell
        let _ = AZURE_CLIENT.set(client);
    }
}

/// Stores a response in Azure Blob Storage using a given cache key.
///
/// # Arguments
/// - `key`: The cache key used as the blob's name.
/// - `data`: The raw response body as bytes.
/// - `headers`: The response headers to store along with the body.
pub async fn store_in_cache(key: String, data: Bytes, headers: Vec<(String, String)>) {
    // Retrieve the global Azure client
    let client = match AZURE_CLIENT.get() {
        Some(c) => c,
        None => {
            error!("Azure client not initialized");
            return;
        }
    };

    // Retrieve the Azure container name from config
    let container = match CONFIG.get() {
        Some(cfg) => cfg.azure_container.clone(),
        None => {
            error!("CONFIG not initialized; cannot read azure_container");
            return;
        }
    };

    // Get blob client from the container and key
    let blob_client = client
        .container_client(container.clone())
        .blob_client(key.clone());

    // Encode the body to base64 and prepare the blob content
    let blob = CachedBlob {
        body: STANDARD.encode(&data),
        headers,
    };

    // Serialize the struct into JSON
    let json = match serde_json::to_vec(&blob) {
        Ok(j) => j,
        Err(e) => {
            error!("❌ Failed to serialize cache for key '{}': {}", key, e);
            return;
        }
    };

    // Upload the blob to Azure
    let result = blob_client
        .put_block_blob(json)
        .content_type("application/json")
        .into_future()
        .await;

    // Log upload result
    match result {
        Ok(_) => info!(
            "✅ Key '{}' stored in Azure Blob Storage container '{}'",
            key, container
        ),
        Err(e) => error!(
            "❌ Failed to store key '{}' in Azure Blob Storage: {}",
            key, e
        ),
    }
}

/// Retrieves cached data from Azure Blob Storage for a given key.
///
/// # Arguments
/// - `key`: The cache key (blob name) to retrieve.
///
/// # Returns
/// - `Some(Bytes, headers)` on success
/// - `None` if the blob was not found or deserialization failed
pub async fn load_from_cache(key: &str) -> Option<(Bytes, Vec<(String, String)>)> {
    let client = AZURE_CLIENT.get()?; // Get Azure client
    let container = CONFIG.get()?.azure_container.clone(); // Get container name

    let blob_client = client
        .container_client(container.clone())
        .blob_client(key);

    // Attempt to download the blob content
    match blob_client.get_content().await {
        Ok(data) => {
            info!(
                "📦 Key '{}' loaded from Azure Blob Storage container '{}'",
                key, container
            );

            // Attempt to deserialize the JSON-encoded CachedBlob
            match serde_json::from_slice::<CachedBlob>(&data) {
                Ok(blob) => {
                    // Decode the base64-encoded body
                    match STANDARD.decode(&blob.body) {
                        Ok(decoded_body) => Some((Bytes::from(decoded_body), blob.headers)),
                        Err(e) => {
                            error!("❌ Failed to decode base64 body for key '{}': {}", key, e);
                            None
                        }
                    }
                }
                Err(e) => {
                    error!("❌ Failed to parse JSON cache for key '{}': {}", key, e);
                    None
                }
            }
        }
        Err(e) => {
            warn!(
                "⚠️ Failed to load key '{}' from Azure Blob Storage: {}",
                key, e
            );
            None
        }
    }
}
