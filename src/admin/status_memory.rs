use chrono::Utc;
use axum::{Json, response::IntoResponse};
use serde::Serialize;
use crate::memory::memory::MEMORY_CACHE;
use crate::config::CONFIG;
use std::collections::HashMap;

#[derive(Serialize)]
pub struct CacheEntry {
    pub inserted_at: String,
    pub size_bytes: usize,
    pub ttl_remaining_secs: i64,
}

pub async fn get_memory_cache_status() -> impl IntoResponse {
    let cache = MEMORY_CACHE.read().await;
    let now = Utc::now();

    // Read TTL from config
    let ttl_secs = CONFIG
        .get()
        .map(|c| c.cache.ttl_seconds)
        .unwrap_or(300); // default fallback

    let entries: HashMap<String, CacheEntry> = cache
        .iter()
        .map(|(key, value)| {
            let elapsed = now.signed_duration_since(value.inserted_at).num_seconds();
            let ttl_remaining = ttl_secs as i64 - elapsed;

            (
                key.clone(),
                CacheEntry {
                    inserted_at: value.inserted_at.to_rfc3339(),
                    size_bytes: value.body.len(),
                    ttl_remaining_secs: ttl_remaining.max(0),
                },
            )
        })
        .collect();

    Json(entries)
}
