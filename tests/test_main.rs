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
    use std::fs;
    use std::sync::Once;
    use cachebolt::config::{Config, StorageBackend};
    
    struct Args {
        config: String,
    }

    static INIT: Once = Once::new();

    fn write_temp_config(contents: &str, filename: &str) -> String {
        let path = format!("tests/{}", filename);
        fs::create_dir_all("tests").ok();
        fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn test_valid_config_parsing() {
        let yaml = r#"
app_id: test-app
gcs_bucket: test-gcs
s3_bucket: test-s3
azure_container: test-az
max_concurrent_requests: 10
downstream_base_url: http://localhost
downstream_timeout_secs: 5
cache:
  memory_threshold: 80
  refresh_percentage: 10
latency_failover:
  default_max_latency_ms: 250
  path_rules:
    - pattern: "^/api/test"
      max_latency_ms: 100
storage_backend: s3
"#;
        let path = write_temp_config(yaml, "valid_config.yaml");
        let cfg = Config::from_file(&path).expect("Config should parse");

        assert_eq!(cfg.app_id, "test-app");
        assert_eq!(cfg.cache.memory_threshold, 80);
        assert_eq!(cfg.latency_failover.path_rules.len(), 1);
        assert_eq!(cfg.storage_backend, StorageBackend::S3);
    }

    #[test]
    fn test_invalid_gcs_config_fails() {
        let yaml = r#"
app_id: fail-app
gcs_bucket: ""
s3_bucket: unused
azure_container: unused
max_concurrent_requests: 10
downstream_base_url: http://localhost
downstream_timeout_secs: 5
cache:
  memory_threshold: 80
  refresh_percentage: 10
latency_failover:
  default_max_latency_ms: 100
  path_rules: []
storage_backend: gcs
"#;
        let path = write_temp_config(yaml, "invalid_config.yaml");
        let result = Config::from_file(&path);
        assert!(result.is_err(), "Expected failure for empty GCS bucket");
    }


  
    #[test]
    fn test_config_from_file_error_handling() {
        let path = "nonexistent_config.yaml";
        let result = Config::from_file(path);
        assert!(result.is_err(), "Should error if config file is missing");
    }
}
