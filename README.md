# ⚡️ CacheBolt

> A blazing-fast reverse proxy with intelligent caching and multi-backend object storage support.

---

## 🚀 Introduction

**CacheBolt** is a high-performance reverse proxy designed to cache and serve responses with minimal latency. It intelligently stores responses in memory and synchronizes them with persistent object storage backends.

This tool is ideal for accelerating APIs, file delivery, and improving reliability under high load.

CacheBolt reads its configuration from a YAML file. By default, it expects a file named:

```bash
./config.yaml
```

You can override this path via CLI:

```bash
./cachebolt --config ./path/to/custom.yaml
```
---

### ✨ Features

- 🔁 Reverse HTTP proxy powered by [Axum](https://github.com/tokio-rs/axum) and [Tokio](https://tokio.rs/)
- 🚀 Fast, concurrent in-memory caching with LRU eviction
- ☁️ Multi-cloud object store support:
  - 🟢 Amazon S3
  - 🔵 Google Cloud Storage
  - 🔶 Azure Blob Storage
  - 💽 Local filesystem
- 📉 Memory-based cache eviction (threshold-configurable)
- ⏱️ Latency-based failover policies (regex route rules)
- 🧠 Smart fallback if upstreams are slow or unavailable

---
## 🔁 Request Flow (Text Diagram)

```text
Client sends GET request
        |
        v
┌────────────────────────────────────────────────────────┐
│            proxy_handler receives request              │
└────────────────────────────────────────────────────────┘
        |
        v
Check if URI is marked as degraded (should_failover)
        |
        ├── Yes --> try_cache(key)
        │            ├── Hit in memory? 
        │            │     └── ✅ Serve from memory
        │            ├── Else: Hit in storage?
        │            │     └── ✅ Load from selected storage backend (GCS, S3, Azure, or Local)
        │            │            └── Load into memory + Serve
        │            └── Else: ❌ Return 502 (no cache, no backend)
        │
        └── No
             |
             v
      Check MEMORY_CACHE for key
             |
             ├── Hit --> ✅ Serve from memory
             └── Miss
                  |
                  v
         Acquire semaphore (concurrency guard)
                  |
                  ├── Denied --> Check memory again
                  │               ├── Hit --> ✅ Serve
                  │               └── ❌ Return 502 (overloaded)
                  |
                  └── Acquired --> forward_request to backend
                                   |
                                   ├── Response latency > threshold?
                                   │         └── Yes --> mark_latency_fail
                                   |
                                   ├── Downstream OK?
                                   │         |
                                   │         ├── Build CachedResponse
                                   │         ├── In failover mode?
                                   │         │     ├── Yes --> Skip caching
                                   │         │     └── No:
                                   │         │           ├── Put in MEMORY_CACHE
                                   │         │           └── Send to CACHE_WRITER (persist to backend)
                                   │         └── ✅ Return response
                                   |
                                   └── Downstream failed --> try_cache fallback
```


---
## 🔧 Configuration

The config is written in YAML. Example:

```yaml
app_id: my-service

max_concurrent_requests: 200
downstream_base_url: http://localhost:4000
downstream_timeout_secs: 5

storage_backend: s3  # options: gcs, s3, azure, local
gcs_bucket: cachebolt
s3_bucket: my-cachebolt-bucket
azure_container: cachebolt-container

memory_eviction:
  threshold_percent: 90

latency_failover:
  default_max_latency_ms: 300
  path_rules:
    - pattern: "^/api/v1/products/.*"
      max_latency_ms: 150
    - pattern: "^/auth/.*"
      max_latency_ms: 100
```

---

## 🔐 Cloud Storage Authentication

Depending on the storage backend, you'll need to configure credentials via environment variables:

### Google Cloud Storage (GCS)
- Must be authenticated using Application Default Credentials (ADC), which you can set via:
```bash
export GOOGLE_APPLICATION_CREDENTIALS="/path/to/service-account.json"
```

### Amazon S3
- Required environment variables:
```bash
export AWS_ACCESS_KEY_ID=your-access-key-id
export AWS_SECRET_ACCESS_KEY=your-secret-key
export AWS_REGION=us-east-1  # or your specific region
```

### Azure Blob Storage
- Required environment variables:
```bash
export AZURE_STORAGE_ACCOUNT=your_account_name
export AZURE_STORAGE_ACCESS_KEY=your_access_key
```

### Local Filesystem
- No additional credentials required. Cache files will be saved locally.

---

## ▶️ Running the Binary

Default mode:
```bash
./cachebolt
```

Custom config path:
```bash
./cachebolt --config ./config/prod.yaml
```

Docker:
```bash
docker run --rm -p 3000:3000 \
  -v $(pwd)/config:/config \
  -v $(pwd)/cache:/data \
  -e GOOGLE_APPLICATION_CREDENTIALS=/config/adc.json \
  ghcr.io/<your-org>/cachebolt:latest \
  --config /config/config.yaml
```

---

## 📦 Building

To build locally:
```bash
cargo build --release
```

To cross-compile:
See `.github/workflows/release.yml` for cross-target examples.

---

## 📄 License

Licensed under the [Apache License 2.0](./LICENSE).
