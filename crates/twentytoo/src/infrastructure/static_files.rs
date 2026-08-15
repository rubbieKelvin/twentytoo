//! Embedded static assets: the file service behind `/static` (`00` §8.6).
//!
//! Assets compile into the binary (`build.rs` generates the name → bytes
//! table from `web/static`), so serving one never touches the filesystem.
//! This module is the lookup + MIME layer; the axum handler lives in
//! `presentation/handlers`.

/// One embedded asset: bytes plus content type.
#[derive(Clone, Copy, Debug)]
pub struct StaticFile {
    /// Asset bytes, owned by the binary.
    pub data: &'static [u8],
    /// The `Content-Type` value.
    pub content_type: &'static str,
}

/// The generated name → bytes table (`build.rs`).
mod generated {
    include!(concat!(env!("OUT_DIR"), "/static_files.rs"));
}

/// The framework static-file service.
pub struct StaticFiles;

impl StaticFiles {
    /// Framework assets the built-in templates reference. The boot check
    /// (`container.rs`) verifies each one is embedded, and the unit tests
    /// assert they stay that way. When a built-in template references a
    /// new asset, add its name here.
    pub const BUILTIN_ASSETS: &[&str] = &[
        "css/tokens.css",
        "css/base.css",
        "css/layout.css",
        "css/components.css",
        "css/utilities.css",
        "js/htmx.min.js",
        "js/app.js",
    ];

    /// Resolve a request path (`css/app.css`, optionally `/`-prefixed) to
    /// its embedded asset.
    ///
    /// Lookup is an exact match against the embedded table — there is no
    /// filesystem to escape into, and traversal names simply miss.
    /// Unknown names return `None` (the handler answers 404).
    pub fn get(path: &str) -> Option<StaticFile> {
        let name = path.strip_prefix('/').unwrap_or(path);
        let (_, data) = generated::ASSETS
            .iter()
            .find(|(name_, _)| return *name_ == name)?;
        return Some(StaticFile {
            data,
            content_type: content_type_for(name),
        });
    }
}

/// Content type for a file extension; unknown types are served as octet
/// streams (`00` §8.6).
fn content_type_for(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("");
    return match ext {
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "txt" => "text/plain; charset=utf-8",
        "html" => "text/html; charset=utf-8",
        _ => "application/octet-stream",
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn css_serves_as_text_css() {
        let file = StaticFiles::get("css/tokens.css").expect("tokens.css embedded");
        assert_eq!(file.content_type, "text/css; charset=utf-8");
        assert!(!file.data.is_empty());
    }

    #[test]
    fn leading_slash_is_normalized() {
        let file = StaticFiles::get("/js/htmx.min.js").expect("htmx embedded");
        assert_eq!(file.content_type, "text/javascript; charset=utf-8");
        assert!(!file.data.is_empty());
    }

    #[test]
    fn unknown_extension_is_octet_stream() {
        assert_eq!(content_type_for("css/app.xyz"), "application/octet-stream");
    }

    #[test]
    fn missing_and_traversal_names_miss() {
        assert!(StaticFiles::get("css/missing.css").is_none());
        assert!(StaticFiles::get("").is_none());
        assert!(StaticFiles::get("../Cargo.toml").is_none());
        assert!(StaticFiles::get("../../etc/passwd").is_none());
    }

    #[test]
    fn builtin_assets_resolve() {
        for name in StaticFiles::BUILTIN_ASSETS {
            assert!(
                StaticFiles::get(name).is_some(),
                "built-in asset missing from the binary: {name}"
            );
        }
    }
}
