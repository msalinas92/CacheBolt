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
use tracing::{info, debug};

use crate::memory::memory::{MEMORY_CACHE, maybe_evict_if_needed, get_memory_usage_kib};

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
pub fn start_background_eviction_task() {
    task::spawn(async {
        // Holds the last known memory usage in percentage
        let mut last_usage_percent = 0;

        loop {
            // Query current used and total memory in KiB
            let (used_kib, total_kib) = get_memory_usage_kib();
            let current_percent = used_kib * 100 / total_kib;

            // Trigger eviction only if memory usage has increased
            if current_percent > last_usage_percent {
                debug!(
                    "📈 Memory usage increased from {}% to {}%, attempting eviction...",
                    last_usage_percent, current_percent
                );

                // Lock the shared LRU cache and attempt eviction based on thresholds
                let mut cache = MEMORY_CACHE.write().await;
                maybe_evict_if_needed(&mut cache).await;
            }

            // Update last usage tracker
            last_usage_percent = current_percent;

            // Sleep 1 second between checks to prevent busy-waiting
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });

    info!("🧠 Background memory eviction task started to monitor usage fluctuations");
}
