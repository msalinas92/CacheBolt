use axum::{
    extract::Path,
    http::{header, Response, StatusCode},
    response::IntoResponse,
};
use mime_guess::from_path;
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "ui/dist/cb-admin/"] // Ruta relativa al Cargo.toml
pub struct EmbeddedAssets;

/// Servidor de archivos embebidos para `/cb-admin/*path`
/// Soporta rutas como:
/// - `/cb-admin`
/// - `/cb-admin/`
/// - `/cb-admin/index.html`
/// - `/cb-admin/cache` -> `cache/index.html`
/// - `/cb-admin/cache/` -> `cache/index.html`
pub async fn embedded_ui_handler(Path(path): Path<String>) -> impl IntoResponse {
    tracing::info!("📦 UI embedded request for: {}", path);

    // Elimina "/" inicial para estandarizar
    let clean_path = path.trim_start_matches('/');

    // Lógica para resolver correctamente rutas tipo `/cb-admin/cache`
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

    // Buscar el archivo embebido
    match EmbeddedAssets::get(&resolved_path) {
        Some(content) => {
            let mime = from_path(&resolved_path).first_or_octet_stream();
            Response::builder()
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(axum::body::Body::from(content.data.into_owned()))
                .unwrap()
        }
        None => {
            // Fallback a index.html para SPA si existe
            if let Some(index) = EmbeddedAssets::get("index.html") {
                return Response::builder()
                    .header(header::CONTENT_TYPE, "text/html")
                    .body(axum::body::Body::from(index.data.into_owned()))
                    .unwrap();
            }

            // Si ni siquiera hay index, error 404
            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(axum::body::Body::from("404 Not Found"))
                .unwrap()
        }
    }
}

/// Sirve el archivo `index.html` directamente para rutas `/cb-admin` o `/cb-admin/`
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
