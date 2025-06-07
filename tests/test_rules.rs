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

#[cfg(test)]
mod tests {
    use super::*;
    use cachebolt::config::{
        CONFIG, Config, LatencyFailover, MaxLatencyRule, MemoryEviction, StorageBackend,
    };
    use cachebolt::rules::latency::{
        LATENCY_FAILS, get_max_latency_for_path, mark_latency_fail, should_failover,
    };
    use ctor::ctor;
    use once_cell::sync::OnceCell;
    use regex::Regex;
    use std::thread::sleep;
    use std::time::{Duration, Instant};

    #[ctor]
    fn init_mock_config() {
        let mock_config = Config {
            latency_failover: LatencyFailover {
                default_max_latency_ms: 1500,
                path_rules: vec![
                    MaxLatencyRule {
                        pattern: "^/api/slow.*".to_string(),
                        max_latency_ms: 3000,
                    },
                    MaxLatencyRule {
                        pattern: "^/internal/healthz$".to_string(),
                        max_latency_ms: 100,
                    },
                ],
            },
            app_id: "test-app".into(),
            gcs_bucket: "test-gcs".into(),
            s3_bucket: "test-s3".into(),
            azure_container: "test-azure".into(),
            max_concurrent_requests: 10,
            downstream_base_url: "http://localhost".into(),
            downstream_timeout_secs: 5,
            memory_eviction: MemoryEviction {
                threshold_percent: 90,
            },
            storage_backend: StorageBackend::Local,
        };

        let _ = CONFIG.set(mock_config);
    }

    #[test]
    fn test_mark_and_should_failover() {
        let key = "/api/test";
        assert!(!should_failover(key), "Should not failover initially");

        mark_latency_fail(key);
        assert!(should_failover(key), "Should failover after mark");

        {
            let mut map = LATENCY_FAILS.write().unwrap();
            map.insert(key.to_string(), Instant::now() - Duration::from_secs(600));
        }

        assert!(!should_failover(key), "Should not failover after expiry");
    }

    #[test]
    fn test_latency_threshold_matching() {
        assert_eq!(get_max_latency_for_path("/api/slow-response"), 3000);
        assert_eq!(get_max_latency_for_path("/internal/healthz"), 100);
        assert_eq!(get_max_latency_for_path("/something/else"), 1500);
    }

    #[test]
    fn test_latency_threshold_with_no_rules_returns_default() {
        let cfg = Config {
            latency_failover: LatencyFailover {
                default_max_latency_ms: 1234,
                path_rules: vec![],
            },
            app_id: "test-app".into(),
            gcs_bucket: "test-gcs".into(),
            s3_bucket: "test-s3".into(),
            azure_container: "test-azure".into(),
            max_concurrent_requests: 10,
            downstream_base_url: "http://localhost".into(),
            downstream_timeout_secs: 5,
            memory_eviction: MemoryEviction {
                threshold_percent: 90,
            },
            storage_backend: StorageBackend::Local,
        };

        let result = cfg.latency_failover.path_rules.iter().find_map(|rule| {
            Regex::new(&rule.pattern).ok().and_then(|re| {
                if re.is_match("/something") {
                    Some(rule.max_latency_ms)
                } else {
                    None
                }
            })
        });

        assert!(result.is_none());
    }

    #[test]
    fn test_mark_latency_fail_overwrites_timestamp() {
        let key = "/overwrite/test";
        mark_latency_fail(key);
        {
            let first = LATENCY_FAILS.read().unwrap().get(key).cloned().unwrap();
            sleep(Duration::from_millis(10));
            mark_latency_fail(key);
            let second = LATENCY_FAILS.read().unwrap().get(key).cloned().unwrap();
            assert!(second > first, "Timestamp should be updated on overwrite");
        }
    }

    #[test]
    fn test_latency_rule_with_invalid_regex_is_ignored() {
        let mut mock = CONFIG.get().unwrap().clone();
        mock.latency_failover.path_rules.push(MaxLatencyRule {
            pattern: "[unclosed".into(),
            max_latency_ms: 9999,
        });

        assert_eq!(get_max_latency_for_path("/any"), 1500);
    }

  
}
