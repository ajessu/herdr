use axum::extract::Path;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::IntoResponse;
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "web-assets/"]
struct Assets;

pub(crate) async fn serve_index(headers: HeaderMap) -> impl IntoResponse {
    serve("index.html", "text/html; charset=utf-8", &headers)
}

pub(crate) async fn serve_asset(Path(path): Path<String>, headers: HeaderMap) -> impl IntoResponse {
    serve(&path, mime_for_path(&path), &headers)
}

/// Serve an embedded file with ETag-based revalidation. Asset filenames are not
/// content-hashed, so a herdr upgrade can change the bytes at the same path;
/// `no-cache` forces the browser to revalidate and the ETag lets us answer 304
/// when nothing changed, avoiding both stale assets and needless transfers.
fn serve(path: &str, content_type: &'static str, headers: &HeaderMap) -> axum::response::Response {
    let Some(file) = Assets::get(path) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let etag = etag_for(&file.metadata.sha256_hash());

    if not_modified(headers, &etag) {
        return (
            StatusCode::NOT_MODIFIED,
            [
                (header::ETAG, etag.as_str()),
                (header::CACHE_CONTROL, "no-cache"),
            ],
        )
            .into_response();
    }

    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "no-cache"),
            (header::ETAG, etag.as_str()),
            // Serve the declared MIME type verbatim; never let the browser
            // sniff embedded assets into a different content type.
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        file.data.to_vec(),
    )
        .into_response()
}

/// Whether the client's `If-None-Match` matches our ETag. Strict comparison
/// only — a weak validator (`W/"..."`) or comma-separated list yields a fresh
/// 200, never a stale asset.
fn not_modified(headers: &HeaderMap, etag: &str) -> bool {
    headers
        .get(header::IF_NONE_MATCH)
        .is_some_and(|value| value.as_bytes() == etag.as_bytes())
}

fn etag_for(hash: &[u8; 32]) -> String {
    use std::fmt::Write as _;
    // 2 quotes + 16 hex chars for the first 8 bytes of the hash.
    let mut etag = String::with_capacity(2 + 16);
    etag.push('"');
    for byte in &hash[..8] {
        let _ = write!(etag, "{byte:02x}");
    }
    etag.push('"');
    etag
}

fn mime_for_path(path: &str) -> &'static str {
    if path.ends_with(".js") {
        "application/javascript; charset=utf-8"
    } else if path.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if path.ends_with(".html") {
        "text/html; charset=utf-8"
    } else if path.ends_with(".woff2") {
        "font/woff2"
    } else if path.ends_with(".woff") {
        "font/woff"
    } else if path.ends_with(".svg") {
        "image/svg+xml"
    } else if path.ends_with(".png") {
        "image/png"
    } else {
        "application/octet-stream"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_for_known_extensions() {
        assert_eq!(
            mime_for_path("terminal.js"),
            "application/javascript; charset=utf-8"
        );
        assert_eq!(mime_for_path("style.css"), "text/css; charset=utf-8");
        assert_eq!(mime_for_path("index.html"), "text/html; charset=utf-8");
        assert_eq!(mime_for_path("xterm/font.woff2"), "font/woff2");
        assert_eq!(mime_for_path("xterm/font.woff"), "font/woff");
        assert_eq!(mime_for_path("icon.svg"), "image/svg+xml");
        assert_eq!(mime_for_path("logo.png"), "image/png");
    }

    #[test]
    fn mime_for_unknown_extension() {
        assert_eq!(mime_for_path("data.bin"), "application/octet-stream");
        assert_eq!(mime_for_path("noextension"), "application/octet-stream");
    }

    #[test]
    fn index_html_is_embedded() {
        assert!(
            Assets::get("index.html").is_some(),
            "index.html must be embedded from web-assets/"
        );
    }

    #[test]
    fn vendored_xterm_is_embedded() {
        assert!(Assets::get("xterm/xterm.min.js").is_some());
        assert!(Assets::get("xterm/xterm.css").is_some());
    }

    #[test]
    fn etag_is_quoted_hex_and_stable() {
        let hash = [0xabu8; 32];
        let etag = etag_for(&hash);
        assert_eq!(etag, "\"abababababababab\"");
        assert_eq!(etag_for(&hash), etag);
    }

    #[test]
    fn etag_differs_for_different_hashes() {
        let a = etag_for(&[0x00u8; 32]);
        let b = etag_for(&[0xffu8; 32]);
        assert_ne!(a, b);
    }

    #[test]
    fn not_modified_matches_exact_etag() {
        let etag = etag_for(&[0x11u8; 32]);
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, etag.parse().unwrap());
        assert!(not_modified(&headers, &etag));
    }

    #[test]
    fn not_modified_rejects_mismatch_and_absence() {
        let etag = etag_for(&[0x11u8; 32]);
        let mut headers = HeaderMap::new();
        assert!(
            !not_modified(&headers, &etag),
            "absent header is a fresh GET"
        );
        headers.insert(
            header::IF_NONE_MATCH,
            "\"deadbeefdeadbeef\"".parse().unwrap(),
        );
        assert!(
            !not_modified(&headers, &etag),
            "mismatched etag is a fresh GET"
        );
    }
}
