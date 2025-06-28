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
pub mod tests {
    use cachebolt::config::{Config, LatencyFailover, CacheSettings, StorageBackend, CONFIG};
    use std::env;
    use std::fs::write;

    fn temp_config_path(filename: &str) -> String {
        let dir = env::temp_dir();
        dir.join(filename).to_string_lossy().to_string()
    }

    #[test]
    fn test_load_valid_config_from_file() {
        let yaml = r#"
app_id: testapp
gcs_bucket: test-gcs
s3_bucket: test-s3
azure_container: test-azure
max_concurrent_requests: 10
downstream_base_url: http://localhost
downstream_timeout_secs: 5
cache:
  memory_threshold: 75
  refresh_percentage: 10
latency_failover:
  default_max_latency_ms: 300
  path_rules:
    - pattern: ^/api/test
      max_latency_ms: 100
storage_backend: s3
"#;

        let path = temp_config_path("valid_config.yaml");
        write(&path, yaml).unwrap();
        let config = Config::from_file(&path).expect("should parse valid config");

        assert_eq!(config.app_id, "testapp");
        assert_eq!(config.cache.memory_threshold, 75);
        assert_eq!(config.latency_failover.default_max_latency_ms, 300);
        assert_eq!(config.latency_failover.path_rules.len(), 1);
        assert_eq!(config.storage_backend, StorageBackend::S3);
    }

    #[test]
    fn test_missing_gcs_bucket_when_using_gcs_backend() {
        let yaml = r#"
app_id: testapp
gcs_bucket: ""
s3_bucket: test-s3
azure_container: test-azure
max_concurrent_requests: 5
downstream_base_url: http://localhost
cache:
  memory_threshold: 75
  refresh_percentage: 10
latency_failover:
  default_max_latency_ms: 200
  path_rules: []
storage_backend: gcs
"#;

        let path = temp_config_path("invalid_gcs_config.yaml");
        write(&path, yaml).unwrap();
        let result = Config::from_file(&path);
        assert!(result.is_err(), "Expected error due to empty gcs_bucket");
    }

    #[test]
    fn test_latency_failover_default_only() {
        let yaml = r#"
app_id: testapp
gcs_bucket: ""
s3_bucket: test-s3
azure_container: test-azure
max_concurrent_requests: 3
downstream_base_url: http://localhost
downstream_timeout_secs: 2
cache:
  memory_threshold: 75
  refresh_percentage: 10
latency_failover:
  default_max_latency_ms: 150
  path_rules: []
storage_backend: local
"#;

        let path = temp_config_path("latency_only.yaml");
        write(&path, yaml).unwrap();
        let config = Config::from_file(&path).unwrap();
        assert_eq!(config.latency_failover.path_rules.len(), 0);
        assert_eq!(config.latency_failover.default_max_latency_ms, 150);
    }

    #[test]
    fn test_storage_backend_deserialization() {
        let yaml = r#"
app_id: test
gcs_bucket: b1
s3_bucket: b2
azure_container: b3
max_concurrent_requests: 1
downstream_base_url: http://x
downstream_timeout_secs: 1
cache:
  memory_threshold: 75
  refresh_percentage: 10
latency_failover:
  default_max_latency_ms: 100
  path_rules: []
storage_backend: azure
"#;

        let path = temp_config_path("backend_enum.yaml");
        write(&path, yaml).unwrap();
        let config = Config::from_file(&path).unwrap();
        assert_eq!(config.storage_backend, StorageBackend::Azure);
    }

    #[test]
    fn test_manual_config_set() {
        let config = Config {
            app_id: "x".into(),
            gcs_bucket: "g".into(),
            s3_bucket: "s".into(),
            azure_container: "a".into(),
            max_concurrent_requests: 1,
            downstream_base_url: "http://x".into(),
            cache: CacheSettings {
                memory_threshold: 90,
                refresh_percentage: 10,
                ttl_seconds: 300,
            },
            latency_failover: LatencyFailover {
                default_max_latency_ms: 200,
                path_rules: vec![],
            },
            storage_backend: StorageBackend::Local,
            ignored_headers: None,
            proxy_port: 3000,
            admin_port: 3001,
        };

        CONFIG.get_or_init(|| config);

        let actual = CONFIG.get().unwrap();
        assert_eq!(actual.cache.memory_threshold, 90);
        assert_eq!(actual.storage_backend, StorageBackend::Local);
    }

    #[test]
    fn test_nonexistent_file_fails() {
        let result = Config::from_file("nonexistent.yaml");
        assert!(result.is_err());
    }

    #[test]
    fn test_malformed_yaml_fails() {
        let malformed = "app_id: test\n  - invalid_yaml";
        let path = temp_config_path("bad.yaml");
        write(&path, malformed).unwrap();
        let result = Config::from_file(&path);
        assert!(result.is_err());
    }

}