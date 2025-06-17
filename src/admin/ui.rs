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

use axum::{
    extract::Path,
    http::{header, Response, StatusCode},
    response::IntoResponse,
};
use mime_guess::from_path;
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "ui/dist/cb-admin/"] 
pub struct EmbeddedAssets;


pub async fn embedded_ui_handler(Path(path): Path<String>) -> impl IntoResponse {
    tracing::info!("📦 UI embedded request for: {}", path);

    let clean_path = path.trim_start_matches('/');

    /// Determines the appropriate asset path to serve based on the provided `clean_path`.
    ///
    /// - If `clean_path` is empty, defaults to `"index.html"`.
    /// - If an asset exists for `clean_path`, uses it directly.
    /// - Otherwise, checks if an asset exists for `"{clean_path}/index.html"` and uses it if available.
    /// - If none of the above, falls back to using `clean_path` as is.
    ///
    /// This logic ensures that directory requests are resolved to their `index.html`
    /// and that only existing embedded assets are served.
    let resolved_path = if clean_path.is_empty() {
        "index.html".to_string()
    } else if EmbeddedAssets::get(clean_path).is_some() {
        clean_path.to_string()
    } else {
        let with_index = format!("{}/index.html", clean_path);
        if EmbeddedAssets::get(&with_index).is_some() {
            with_index
        } else {
            clean_path.to_string() 
        }
    };

    match EmbeddedAssets::get(&resolved_path) {
        Some(content) => {
            /// Determines the MIME type of the file at the given `resolved_path`.
            /// If the MIME type cannot be determined, defaults to `application/octet-stream`.
            /// 
            /// # Returns
            /// 
            /// A [`mime::Mime`] representing the file's MIME type.
            let mime = from_path(&resolved_path).first_or_octet_stream();
            Response::builder()
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(axum::body::Body::from(content.data.into_owned()))
                .unwrap()
        }
        None => {
            if let Some(index) = EmbeddedAssets::get("index.html") {
                return Response::builder()
                    .header(header::CONTENT_TYPE, "text/html")
                    .body(axum::body::Body::from(index.data.into_owned()))
                    .unwrap();
            }

            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(axum::body::Body::from("404 Not Found"))
                .unwrap()
        }
    }
}

pub async fn embedded_ui_index() -> impl IntoResponse {
    match EmbeddedAssets::get("index.html") {
        Some(content) => Response::builder()
            .header(header::CONTENT_TYPE, "text/html")
            .body(axum::body::Body::from(content.data.into_owned()))
            .unwrap(),
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body("404 Not Found".into())
            .unwrap(),
    }
}
