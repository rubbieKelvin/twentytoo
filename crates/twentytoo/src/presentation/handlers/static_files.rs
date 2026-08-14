//! The `/static/{*path}` handler: embedded assets over HTTP (`00` §8.6).
//!
//! Assets come from the binary, never the filesystem — the request path
//! (`css/app.css`) is an exact lookup against the embedded table. Unknown
//! names answer a plain 404, not the HTML fallback page.

use axum::extract::Path;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::infrastructure::static_files::StaticFiles;

/// Serve one embedded asset.
pub async fn static_file_handler(Path(path): Path<String>) -> Response {
    return match StaticFiles::get(&path) {
        Some(file) => ([(header::CONTENT_TYPE, file.content_type)], file.data).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    };
}
