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

use hyper::HeaderMap;

/// Returns true if the client explicitly requests to bypass *all* cache layers.
///
/// When true:
/// - The cache will be skipped for read and write.
/// - The backend will be hit directly.
pub fn should_bypass_cache(headers: &HeaderMap) -> bool {
    if let Some(value) = headers.get("cache-control") {
        if value.to_str().unwrap_or("").to_ascii_lowercase().contains("no-cache") {
            return true;
        }
    }

    if let Some(value) = headers.get("x-bypass-cache") {
        if value.to_str().unwrap_or("").to_ascii_lowercase() == "true" {
            return true;
        }
    }

    false
}