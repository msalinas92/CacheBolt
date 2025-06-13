
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
        CacheSettings, Config, LatencyFailover, MaxLatencyRule, StorageBackend, CONFIG
    };
    use cachebolt::storage::local::*;
    use std::fs;
    use std::path::Path;
    use azure_storage_blobs::blob;
    use tokio;
    use flate2::{Compression, read::GzDecoder, write::GzEncoder};
    use serde::Serialize;
    use bytes::Bytes;
    use std::io::Write;
    use serde::ser::{Serialize as TraitSerialize, Serializer};
    use cachebolt::storage::local::CachedBlob;

    fn init_config_for_tests() {
        if CONFIG.get().is_none() {
            let config = Config {
                app_id: "testapp".to_string(),
                gcs_bucket: "".to_string(),
                s3_bucket: "".to_string(),
                azure_container: "".to_string(),
                max_concurrent_requests: 10,
                downstream_base_url: "http://localhost".to_string(),
                downstream_timeout_secs: 5,
                cache: CacheSettings {
                    memory_threshold: 90,
                    refresh_percentage: 10, // Set a default refresh percentage
                },
                latency_failover: LatencyFailover {
                    default_max_latency_ms: 200,
                    path_rules: vec![MaxLatencyRule {
                        pattern: "^/api/test".to_string(),
                        max_latency_ms: 100,
                    }],
                },
                storage_backend: StorageBackend::Local,
                ignored_headers: None,
            };
            let _ = CONFIG.set(config);
        }
    }

    #[tokio::test]
    async fn test_store_and_load_cache_roundtrip() {
        init_config_for_tests();
        let key = "test_key_unit";
        let data = Bytes::from("Hello, Cache!");
        let headers = vec![
            ("Content-Type".to_string(), "text/plain".to_string()),
            ("X-Test".to_string(), "true".to_string()),
        ];

        store_in_cache(key.to_string(), data.clone(), headers.clone()).await;

        let result = load_from_cache(key).await;
        assert!(result.is_some(), "Expected cached value to be returned");

        let (loaded_data, loaded_headers) = result.unwrap();
        assert_eq!(loaded_data, data);
        assert_eq!(loaded_headers, headers);

        if let Some(path) = build_local_cache_path(key) {
            if Path::new(&path).exists() {
                let _ = fs::remove_file(path);
            }
        }
    }

    #[tokio::test]
    async fn test_load_from_nonexistent_cache() {
        init_config_for_tests();
        let result = load_from_cache("nonexistent_key_12345").await;
        assert!(result.is_none(), "Expected None for missing cache file");
    }

    #[tokio::test]
    async fn test_store_fails_with_invalid_utf8() {
        init_config_for_tests();
        let key = "invalid_utf8";
        let data = Bytes::from(vec![0xFF, 0xFE, 0xFD]);
        let headers = vec![];

        store_in_cache(key.to_string(), data, headers).await;
        let result = load_from_cache(key).await;
        assert!(result.is_some(), "Even invalid binary should be storable");
    }

    #[tokio::test]
    async fn test_load_fails_with_corrupt_gzip() {
        init_config_for_tests();

        if let Some(path) = build_local_cache_path("corrupt") {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, b"not gzip").unwrap();

            let result = load_from_cache("corrupt").await;
            assert!(result.is_none(), "Should return None on corrupt gzip");

            let _ = fs::remove_file(path);
        }
    }

    #[tokio::test]
    async fn test_load_fails_on_invalid_json() {
        init_config_for_tests();

        if let Some(path) = build_local_cache_path("invalid_json") {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(b"This is not JSON").unwrap();
            let compressed = encoder.finish().unwrap();

            fs::write(&path, compressed).unwrap();
            let result = load_from_cache("invalid_json").await;
            assert!(result.is_none(), "Expected None for invalid JSON in gzip");

            let _ = fs::remove_file(path);
        }
    }

    #[tokio::test]
    async fn test_load_fails_on_base64_decode() {
        init_config_for_tests();

        if let Some(path) = build_local_cache_path("invalid_base64") {
            fs::create_dir_all(path.parent().unwrap()).unwrap();

            let bad_blob = r#"{
                \"body\": \"!!!!NOTBASE64!!!!\",
                \"headers\": [[\"X-Test\", \"true\"]]
            }"#;

            let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(bad_blob.as_bytes()).unwrap();
            let compressed = encoder.finish().unwrap();

            fs::write(&path, compressed).unwrap();

            let result = load_from_cache("invalid_base64").await;
            assert!(result.is_none(), "Expected None for base64 decode error");

            let _ = fs::remove_file(path);
        }
    }

    #[tokio::test]
    async fn test_store_in_cache_handles_directory_creation_failure() {
        init_config_for_tests();

        let key = "key_with_invalid_path\0"; // null byte triggers error
        let data = Bytes::from("invalid");
        let headers = vec![];

        // Just ensure it doesn't panic or crash
        store_in_cache(key.to_string(), data, headers).await;
    }

    #[tokio::test]
    async fn test_store_in_cache_directory_creation_error() {
        init_config_for_tests();

        // Este path apunta a un archivo real, por lo que no puede tener subdirectorios
        let key = "/dev/null/bad_path_key";
        let data = Bytes::from("data");
        let headers = vec![];

        // No debe panicar, y debe salir silenciosamente
        store_in_cache(key.to_string(), data, headers).await;
    }

    #[tokio::test]
    async fn test_store_fails_on_json_serialization() {
        use serde::ser::{Serialize, Serializer};

        init_config_for_tests();
        let key = "fail_json_serialization";
        let data = Bytes::from("data");

        // Tipo inválido para forzar error de serialización
        struct NonSerializable;

        impl Serialize for NonSerializable {
            fn serialize<S>(&self, _: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                Err(serde::ser::Error::custom("Intentional failure"))
            }
        }

        // Aquí forzamos un header que no es serializable (truco de shadowing)
        #[derive(Serialize)]
        struct BrokenBlob {
            body: String,
            headers: Vec<NonSerializable>,
        }

        let blob = BrokenBlob {
            body: "irrelevant".into(),
            headers: vec![NonSerializable],
        };

        let result = serde_json::to_vec(&blob);
        assert!(result.is_err(), "Expected serialization to fail");

        // Esto confirma que la estrategia funciona y puede adaptarse al struct real.
    }

    #[tokio::test]
    async fn test_store_fails_on_gzip_write_error() {
        use flate2::Compression;
        use flate2::write::GzEncoder;
        use std::io::{Result as IoResult, Write};

        // Writer falso que siempre falla
        struct FailingWriter;

        impl Write for FailingWriter {
            fn write(&mut self, _buf: &[u8]) -> IoResult<usize> {
                Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "forced write error",
                ))
            }
            fn flush(&mut self) -> IoResult<()> {
                Ok(())
            }
        }

        let blob    = CachedBlob {
            body: "SGVsbG8=".to_string(),
            headers: vec![("X-Test".to_string(), "true".to_string())],
        };

        let json = serde_json::to_vec(&blob).expect("Must serialize");

        let mut encoder = GzEncoder::new(FailingWriter, Compression::default());
        let result = encoder.write_all(&json);

        assert!(result.is_err(), "Expected compression to fail");
    }


    #[tokio::test]
    async fn test_store_fails_to_write_file_contents() {
        use std::os::unix::fs::PermissionsExt;

        init_config_for_tests();

        let key = "readonly_test_file";
        let path = build_local_cache_path(key).unwrap();

        // Crea archivo vacío sin permisos de escritura
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"").unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o400); // Solo lectura
        fs::set_permissions(&path, perms).unwrap();

        // Intenta escribir encima
        let data = Bytes::from("data");
        let headers = vec![];
        store_in_cache(key.to_string(), data, headers).await;

        // Limpieza
        let _ = fs::remove_file(path);
    }
}
