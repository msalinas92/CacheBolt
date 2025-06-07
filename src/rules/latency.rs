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
use regex::Regex;
use std::collections::HashMap;
use once_cell::sync::Lazy;
use std::sync::RwLock;
use std::time::{Duration, Instant};

/// Tracks recent high-latency failures per URI using a shared in-memory map.
/// Used to determine when to activate failover mode for specific routes.
pub static LATENCY_FAILS: Lazy<RwLock<HashMap<String, Instant>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

/// Returns `true` if the given URI has experienced high latency within
/// the last 5 minutes (300 seconds), and should therefore be served
/// from cache to avoid additional pressure on the downstream service.
pub fn should_failover(uri: &str) -> bool {
    let key = uri.to_string();
    let now = Instant::now();
    let map = LATENCY_FAILS.read().unwrap();
    if let Some(&last_fail) = map.get(&key) {
        now.duration_since(last_fail) < Duration::from_secs(300)
    } else {
        false
    }
}

/// Marks a specific URI as having triggered a latency threshold violation.
/// This updates the internal map to record the failure timestamp,
/// which influences future routing decisions (failover).
pub fn mark_latency_fail(uri: &str) {
    let mut map = LATENCY_FAILS.write().unwrap();
    map.insert(uri.to_string(), Instant::now());
}

/// Returns the latency threshold (in milliseconds) for the given URI.
/// If the URI matches a custom regex rule from the config, that threshold
/// is returned. Otherwise, the global default threshold is used.
pub fn get_max_latency_for_path(uri: &str) -> u64 {
    let cfg = CONFIG.get().expect("CONFIG not initialized");
    for rule in &cfg.latency_failover.path_rules {
        if let Ok(re) = Regex::new(&rule.pattern) {
            if re.is_match(uri) {
                return rule.max_latency_ms;
            }
        }
    }
    cfg.latency_failover.default_max_latency_ms
}
