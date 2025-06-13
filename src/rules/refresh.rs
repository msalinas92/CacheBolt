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

use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;

use crate::config::CONFIG;
use tracing::{info, debug};

/// Global hit counters for probabilistic refresh logic
static REFRESH_COUNTERS: Lazy<Mutex<HashMap<String, u64>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Determines if a response should bypass cache and refresh from backend
pub fn should_refresh(key: &str) -> bool {
    let percentage = CONFIG.get().map(|c| c.cache.refresh_percentage).unwrap_or(0);

    if percentage == 0 {
        return false;
    }

    let mut counters = REFRESH_COUNTERS.lock().unwrap();
    let counter = counters.entry(key.to_string()).or_insert(0);
    *counter += 1;

    let modulus = 100 / percentage.max(1);
    let should = *counter % modulus as u64 == 0;

    if should {
        info!("🔄 Refresh triggered for key '{}' after {} hits", key, counter);
    } else {
        debug!("⏩ No refresh for key '{}', current count {}", key, counter);
    }

    should
}
