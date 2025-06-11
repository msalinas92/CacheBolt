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
## 🔁 Request Flow

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
ignored_headers:
  - postman-token
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

## 📊 Prometheus Metrics

CacheBolt exposes Prometheus-compatible metrics at the `/metrics` endpoint on port `3000`. These metrics allow you to monitor request flow, latency thresholds, memory caching, and backend persistence.

### Request Metrics

- `cachebolt_proxy_requests_total{uri}`  
  Total number of proxy requests received, labeled by URI.

- `cachebolt_downstream_failures_total{uri}`  
  Count of downstream request failures per URI.

- `cachebolt_rejected_due_to_concurrency_total{uri}`  
  Requests rejected due to max concurrency being exceeded.

- `cachebolt_failover_total{uri}`  
  Requests served via failover mode due to recent high latency.

### In-Memory Cache Metrics

- `cachebolt_memory_hits_total{uri}`  
  Requests served directly from the in-memory cache.

- `cachebolt_memory_store_total{uri}`  
  Responses stored into the in-memory cache.

- `cachebolt_memory_fallback_hits_total`  
  Failover-mode requests served from memory cache.

### Latency Monitoring

- `cachebolt_proxy_request_latency_ms{uri}`  
  Histogram of proxy request latency in milliseconds.

- `cachebolt_latency_exceeded_ms{uri}`  
  Histogram of requests whose latency exceeded the configured threshold.

- `cachebolt_latency_exceeded_total{uri}`  
  Count of latency threshold violations per URI.

### Persistent Storage Metrics

- `cachebolt_persist_attempts_total{backend}`  
  Number of attempts to persist cache entries into the selected backend.

- `cachebolt_persist_errors_total{backend}`  
  Number of failed attempts to persist cache entries.

- `cachebolt_persistent_fallback_hits_total`  
  Requests served from persistent storage (GCS, S3, Azure, or local) during failover.

- `cachebolt_fallback_miss_total`  
  Count of failover attempts that missed both memory and persistent storage.

---
## 🧹 Cache Invalidation

You can clear the entire cache (both in-memory and persistent storage) using the `/cache?backend=true` endpoint. This is useful when deploying major updates or invalidating stale content globally.

- When `backend=true`, CacheBolt will delete all cache entries stored in:
  - 🟢 Amazon S3
  - 🔵 Google Cloud Storage
  - 🔶 Azure Blob Storage
  - 💽 Local Filesystem

### ✅ Example: Full cache invalidation

```bash
curl -X DELETE "http://localhost:3000/cache?backend=true"
```

This will:

Clear all in-memory cache

Batch-delete all objects under the prefix cache/{app_id}/ from the configured storage backend

On S3, it uses optimized DeleteObjects requests (up to 1000 keys per request)

---

## 📄 License

Licensed under the [Apache License 2.0](./LICENSE).
