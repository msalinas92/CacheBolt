# Pull Request Details

This document provides a detailed description of the changes introduced in this pull request, the motivations behind them, and additional observations found during development.

---

## Testing
I ran the tests using **MinIO**, since I can only deploy storage on Azure and Google.  
It is recommended to also test with **Amazon S3** to confirm compatibility and correctness.

---

## Changes

### `src/config.rs`
- Added `backend_retry_interval_secs` and `storage_backend_failures`.  
  These variables define:
  - The number of failed attempts before the circuit breaker activates.
  - The retry interval to check whether the storage backend is working again.

### `src/main.rs`
- Added calls to initialize new proxy config:
  ```rust
  crate::proxy::init_storage_backend_threshold();
  crate::proxy::init_backend_retry_interval_config();
Ensures variables are read before being used in proxy.rs, stored in static variables, avoiding repeated reads from CONFIG.

Added a println! for startup error reporting, since logging is not yet initialized at that stage.

Observation: config values (like backend_label in CACHE_WRITER) are read directly from CONFIG repeatedly, which may be inefficient.

###  `src/proxy.rs`
Added static variables to manage circuit breaker behavior:

- `BACKEND_RETRY_INTERVAL_SECS_CONFIG`
- `STORAGE_BACKEND_FAILURES_THRESHOLD`
- `BUCKET_ACCESS_ERRORS`

Added `std::sync::atomic::{AtomicUsize, AtomicBool, Ordering}` for multithreaded handling.
Added `functions init_storage_backend_threshold` and `init_backend_retry_interval_config`.
Enhanced error handling in `CACHE_WRITER`, `try_cache`, and `proxy_handler`.
Added `is_bucket_access_error` to differentiate bucket errors from other failures.

### `src/storage/s3.rs`
Added modules:

- `aws_sdk_s3::{Client, config::Builder}` → MinIO support.
- `std::env` → Handle AWS_ENDPOINT_URL for MinIO.
- `tokio::time::{sleep, Duration}` → Circuit breaker timing.
- `crate::proxy::CIRCUIT_BREAKER` → Import circuit breaker variable.
- `std::sync::atomic::Ordering` → Thread synchronization.

Modified `init_s3_client` to:

- Added MinIO compatibility (via force_path_style).
- Added initial bucket connection attempt and error reporting at startup.

Modified `store_in_cache` and `load_from_cache` to add error handling, distinguishing cache misses from S3 failures.

Added functions:

- `check_bucket_connection`
- `start_s3_health_checker`

For circuit breaker recovery checks.

### `src/tests`
Finally, I have updated tests to reflect new config variables: 
- `storage_backend_failures`
- `backend_retry_interval_secs`

## Next Steps

- Add support for GCP, Azure, and local backends.
- Create a generic backend error handling function (current logic is spread across CACHE_WRITER and try_cache).
- Replace repeated CONFIG lookups with statics for efficiency.
- Perform load testing.
- Create tests

## Possible Bugs Identified

Even when storage is healthy, CacheBolt always writes API responses into the cache and still calls the backend API. The cache is only used if the backend fails.

The cache key includes part of the request headers, which means calling the same API/URL from different browsers generates different cache keys. This results in duplicate cache entries for identical responses.

Example:

 Calling sum?a=5&b=7 from different browsers creates different cache entries, even though the result is the same.

Moreover, the api which sums always is called and cachebolt saves the cache in the backend storage, even though the entry cache exists.
