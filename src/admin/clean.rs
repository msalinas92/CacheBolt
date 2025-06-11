use crate::memory::memory::MEMORY_CACHE;
use axum::{extract::Query, http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};

// Individual and full backend deletion
use crate::storage::azure::delete_all_from_cache as delete_all_azure;
use crate::storage::gcs::delete_all_from_cache as delete_all_gcs;
use crate::storage::local::delete_all_from_cache as delete_all_local;
use crate::storage::s3::delete_all_from_cache as delete_all_s3;

#[derive(Deserialize)]
pub struct InvalidateParams {
    pub backend: Option<bool>,
}

#[derive(Serialize)]
struct SuccessResponse {
    message: String,
}

/// DELETE /cache?backend=true
pub async fn invalidate_handler(Query(params): Query<InvalidateParams>) -> impl IntoResponse {
    let backend_enabled = params.backend.unwrap_or(false);

    // 🧠 Clear memory cache
    let mut memory = MEMORY_CACHE.write().await;
    let count = memory.len();
    memory.clear();
    tracing::info!("🧨 Cleared all {count} entries from in-memory cache");

    // ☁️ Optionally clear all backends
    if backend_enabled {
        let futures = vec![
            tokio::spawn(async { delete_all_azure().await }),
            tokio::spawn(async { delete_all_gcs().await }),
            tokio::spawn(async { delete_all_s3().await }),
            tokio::spawn(async { delete_all_local().await }),
        ];

        for task in futures {
            if let Err(e) = task.await {
                tracing::warn!("⚠️ A backend deletion task failed: {:?}", e);
            }
        }

        tracing::info!("🧹 Requested full deletion from all persistent backends");
    }

    let body = Json(SuccessResponse {
        message: if backend_enabled {
            format!("Cleared in-memory cache and requested deletion from all backends")
        } else {
            format!("Cleared in-memory cache only")
        },
    });

    (StatusCode::OK, body)
}
