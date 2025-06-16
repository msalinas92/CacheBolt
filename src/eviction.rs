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

/// Background memory eviction task for CacheBolt based on system memory fluctuations
use std::time::Duration;
use tokio::task;

use crate::memory::memory::{MEMORY_CACHE, get_memory_usage_kib, maybe_evict_if_needed};

/// Launches a continuous background task to monitor system memory usage and
/// perform cache eviction dynamically under pressure.
///
/// The logic operates as follows:
/// - Every second, it reads the current memory usage of the system.
/// - If the current usage (in percent) exceeds the last observed usage,
///   it triggers a check to evict entries from the in-memory LRU cache.
/// - This complements the on-write eviction and adds adaptive behavior under load.
///
/// This mechanism ensures the cache remains efficient and avoids OOM conditions,
/// especially under high traffic or memory contention scenarios.
pub fn start_background_eviction_task_with<F>(get_usage: F)
where
    F: Fn() -> (u64, u64) + Send + Sync + 'static,
{
    task::spawn(async move {
        let mut last_usage_percent = 0;

        loop {
            let (used_kib, total_kib) = get_usage();
            let current_percent = used_kib * 100 / total_kib;

            if current_percent > last_usage_percent {
                let mut cache = MEMORY_CACHE.write().await;
                maybe_evict_if_needed(&mut cache).await;
            }

            last_usage_percent = current_percent;
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });

    tracing::info!("🧠 Background memory eviction task started");
}

// Mantén esta para uso real
pub fn start_background_eviction_task() {
    start_background_eviction_task_with(get_memory_usage_kib);
}
