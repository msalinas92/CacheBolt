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
#[folder = "ui/dist/admin/"] // Ruta relativa al Cargo.toml
pub struct EmbeddedAssets;


pub async fn embedded_ui_handler(Path(path): Path<String>) -> impl IntoResponse {
    tracing::info!("📦 UI embedded request for: {}", path);

    let clean_path = path.trim_start_matches('/');

    let resolved_path = if clean_path.is_empty() {
        "index.html".to_string()
    } else if EmbeddedAssets::get(clean_path).is_some() {
        clean_path.to_string()
    } else {
        let with_index = format!("{}/index.html", clean_path);
        if EmbeddedAssets::get(&with_index).is_some() {
            with_index
        } else {
            clean_path.to_string() // Intento final (puede fallar)
        }
    };

    match EmbeddedAssets::get(&resolved_path) {
        Some(content) => {
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
