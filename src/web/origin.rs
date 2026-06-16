use axum::http::HeaderMap;

pub(crate) fn validate_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get("origin") else {
        return false;
    };
    let Some(host) = headers.get("host") else {
        return false;
    };

    let Ok(origin_str) = origin.to_str() else {
        return false;
    };
    let Ok(host_str) = host.to_str() else {
        return false;
    };

    // RFC 6454: Origin is scheme://host[:port] or the literal "null".
    // Reject schemeless or "null" origins.
    let Some(origin_host) = extract_host_from_origin(origin_str) else {
        return false;
    };
    origin_host == host_str
}

fn extract_host_from_origin(origin: &str) -> Option<&str> {
    origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, HeaderValue};

    fn headers(origin: Option<&str>, host: Option<&str>) -> HeaderMap {
        let mut map = HeaderMap::new();
        if let Some(o) = origin {
            map.insert("origin", HeaderValue::from_str(o).unwrap());
        }
        if let Some(h) = host {
            map.insert("host", HeaderValue::from_str(h).unwrap());
        }
        map
    }

    #[test]
    fn matching_origin_and_host() {
        let h = headers(Some("http://127.0.0.1:7681"), Some("127.0.0.1:7681"));
        assert!(validate_origin(&h));
    }

    #[test]
    fn matching_https_origin() {
        let h = headers(Some("https://example.com:443"), Some("example.com:443"));
        assert!(validate_origin(&h));
    }

    #[test]
    fn mismatched_origin_host() {
        let h = headers(Some("http://evil.com:7681"), Some("127.0.0.1:7681"));
        assert!(!validate_origin(&h));
    }

    #[test]
    fn missing_origin_header() {
        let h = headers(None, Some("127.0.0.1:7681"));
        assert!(!validate_origin(&h));
    }

    #[test]
    fn missing_host_header() {
        let h = headers(Some("http://127.0.0.1:7681"), None);
        assert!(!validate_origin(&h));
    }

    #[test]
    fn different_port_fails() {
        let h = headers(Some("http://127.0.0.1:9999"), Some("127.0.0.1:7681"));
        assert!(!validate_origin(&h));
    }

    #[test]
    fn origin_without_scheme_fails() {
        let h = headers(Some("127.0.0.1:7681"), Some("127.0.0.1:7681"));
        assert!(!validate_origin(&h));
    }

    #[test]
    fn null_origin_fails() {
        let h = headers(Some("null"), Some("127.0.0.1:7681"));
        assert!(!validate_origin(&h));
    }
}
