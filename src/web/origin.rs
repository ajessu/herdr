use axum::http::HeaderMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Origin {
    pub secure: bool,
    pub host: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OriginReject {
    Missing,
    Malformed,
    NotAllowed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OriginDecision {
    Accept,
    Reject {
        reason: OriginReject,
        raw: Option<String>,
    },
}

pub(crate) fn normalize_origin(s: &str) -> Option<Origin> {
    let (secure, rest) = if let Some(rest) = s.strip_prefix("https://") {
        (true, rest)
    } else if let Some(rest) = s.strip_prefix("http://") {
        (false, rest)
    } else if let Some(rest) = s.strip_prefix("HTTPS://") {
        (true, rest)
    } else if let Some(rest) = s.strip_prefix("HTTP://") {
        (false, rest)
    } else {
        let lower = s.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("https://") {
            return normalize_origin_inner(true, rest);
        } else if let Some(rest) = lower.strip_prefix("http://") {
            return normalize_origin_inner(false, rest);
        }
        return None;
    };

    normalize_origin_inner(secure, rest)
}

fn normalize_origin_inner(secure: bool, rest: &str) -> Option<Origin> {
    if rest.is_empty() {
        return None;
    }

    if rest.contains('@') {
        return None;
    }
    if rest.contains('/') || rest.contains('?') || rest.contains('#') {
        return None;
    }

    let host = if rest.starts_with('[') {
        let close = rest.find(']')?;
        let bracket_content = &rest[1..close];
        if bracket_content.contains('%') {
            return None;
        }
        let after = &rest[close + 1..];
        if after.is_empty() {
            rest.to_ascii_lowercase()
        } else if let Some(port_str) = after.strip_prefix(':') {
            let port: u16 = port_str.parse().ok()?;
            if port == 0 {
                return None;
            }
            let default_port = if secure { 443 } else { 80 };
            if port == default_port {
                rest[..close + 1].to_ascii_lowercase()
            } else {
                format!("{}:{}", rest[..close + 1].to_ascii_lowercase(), port)
            }
        } else {
            return None;
        }
    } else {
        if rest.ends_with('.') {
            return None;
        }

        let (hostname, port_part) = if let Some(colon) = rest.rfind(':') {
            let maybe_port = &rest[colon + 1..];
            if maybe_port.is_empty() {
                return None;
            }
            if let Ok(port) = maybe_port.parse::<u16>() {
                if port == 0 {
                    return None;
                }
                let default_port = if secure { 443 } else { 80 };
                if port == default_port {
                    (&rest[..colon], None)
                } else {
                    (&rest[..colon], Some(port))
                }
            } else {
                return None;
            }
        } else {
            (rest, None)
        };

        if hostname.is_empty() {
            return None;
        }

        let lower_host = hostname.to_ascii_lowercase();
        match port_part {
            Some(port) => format!("{lower_host}:{port}"),
            None => lower_host,
        }
    };

    Some(Origin { secure, host })
}

pub(crate) fn validate_origin(
    headers: &HeaderMap,
    allowed: &[Origin],
    allow_same_origin: bool,
) -> OriginDecision {
    let Some(origin_header) = headers.get("origin") else {
        return OriginDecision::Reject {
            reason: OriginReject::Missing,
            raw: None,
        };
    };

    let Ok(origin_str) = origin_header.to_str() else {
        return OriginDecision::Reject {
            reason: OriginReject::Malformed,
            raw: None,
        };
    };

    let raw = origin_str.to_owned();

    let Some(parsed) = normalize_origin(origin_str) else {
        return OriginDecision::Reject {
            reason: OriginReject::Malformed,
            raw: Some(raw),
        };
    };

    if allowed.iter().any(|a| *a == parsed) {
        return OriginDecision::Accept;
    }

    if allow_same_origin {
        if let Some(host_header) = headers.get("host") {
            if let Ok(host_str) = host_header.to_str() {
                let same_origin = Origin {
                    secure: parsed.secure,
                    host: host_str.to_ascii_lowercase(),
                };
                if parsed == same_origin {
                    return OriginDecision::Accept;
                }
            }
        }
    }

    OriginDecision::Reject {
        reason: OriginReject::NotAllowed,
        raw: Some(raw),
    }
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

    // --- normalize_origin tests ---

    #[test]
    fn normalize_http_no_port() {
        let o = normalize_origin("http://example.com").unwrap();
        assert!(!o.secure);
        assert_eq!(o.host, "example.com");
    }

    #[test]
    fn normalize_https_no_port() {
        let o = normalize_origin("https://example.com").unwrap();
        assert!(o.secure);
        assert_eq!(o.host, "example.com");
    }

    #[test]
    fn normalize_default_port_dropped_http() {
        let o = normalize_origin("http://example.com:80").unwrap();
        assert_eq!(o.host, "example.com");
    }

    #[test]
    fn normalize_default_port_dropped_https() {
        let o = normalize_origin("https://example.com:443").unwrap();
        assert_eq!(o.host, "example.com");
    }

    #[test]
    fn normalize_non_default_port_preserved() {
        let o = normalize_origin("https://example.com:8443").unwrap();
        assert_eq!(o.host, "example.com:8443");
    }

    #[test]
    fn normalize_host_lowercased() {
        let o = normalize_origin("https://Example.COM").unwrap();
        assert_eq!(o.host, "example.com");
    }

    #[test]
    fn normalize_uppercase_scheme() {
        let o = normalize_origin("HTTPS://Example.COM").unwrap();
        assert!(o.secure);
        assert_eq!(o.host, "example.com");
    }

    #[test]
    fn normalize_schemeless_rejected() {
        assert!(normalize_origin("example.com").is_none());
    }

    #[test]
    fn normalize_null_rejected() {
        assert!(normalize_origin("null").is_none());
    }

    #[test]
    fn normalize_empty_host_rejected() {
        assert!(normalize_origin("http://").is_none());
    }

    #[test]
    fn normalize_trailing_dot_host_rejected() {
        assert!(normalize_origin("http://example.com.").is_none());
    }

    #[test]
    fn normalize_userinfo_rejected() {
        assert!(normalize_origin("http://user:pass@example.com").is_none());
    }

    #[test]
    fn normalize_path_rejected() {
        assert!(normalize_origin("http://example.com/path").is_none());
    }

    #[test]
    fn normalize_query_rejected() {
        assert!(normalize_origin("http://example.com?q=1").is_none());
    }

    #[test]
    fn normalize_fragment_rejected() {
        assert!(normalize_origin("http://example.com#frag").is_none());
    }

    #[test]
    fn normalize_zero_port_rejected() {
        assert!(normalize_origin("http://example.com:0").is_none());
    }

    #[test]
    fn normalize_ipv6_literal() {
        let o = normalize_origin("http://[::1]:8080").unwrap();
        assert_eq!(o.host, "[::1]:8080");
    }

    #[test]
    fn normalize_ipv6_default_port_dropped() {
        let o = normalize_origin("http://[::1]:80").unwrap();
        assert_eq!(o.host, "[::1]");
    }

    #[test]
    fn normalize_ipv6_zone_id_rejected() {
        assert!(normalize_origin("http://[fe80::1%25eth0]:80").is_none());
    }

    #[test]
    fn normalize_default_port_equivalence() {
        let a = normalize_origin("https://host").unwrap();
        let b = normalize_origin("https://host:443").unwrap();
        assert_eq!(a, b);
    }

    // --- validate_origin tests ---

    #[test]
    fn allowlisted_origin_accepted() {
        let allowed = vec![normalize_origin("https://test-origin.com").unwrap()];
        let h = headers(Some("https://test-origin.com"), Some("localhost:7681"));
        assert_eq!(validate_origin(&h, &allowed, false), OriginDecision::Accept);
    }

    #[test]
    fn non_allowlisted_origin_rejected() {
        let allowed = vec![normalize_origin("https://test-origin.com").unwrap()];
        let h = headers(Some("https://evil.com"), Some("localhost:7681"));
        assert!(matches!(
            validate_origin(&h, &allowed, false),
            OriginDecision::Reject {
                reason: OriginReject::NotAllowed,
                ..
            }
        ));
    }

    #[test]
    fn missing_origin_rejected() {
        let h = headers(None, Some("localhost:7681"));
        assert!(matches!(
            validate_origin(&h, &[], false),
            OriginDecision::Reject {
                reason: OriginReject::Missing,
                ..
            }
        ));
    }

    #[test]
    fn malformed_origin_rejected() {
        let h = headers(Some("not-a-url"), Some("localhost:7681"));
        assert!(matches!(
            validate_origin(&h, &[], false),
            OriginDecision::Reject {
                reason: OriginReject::Malformed,
                ..
            }
        ));
    }

    #[test]
    fn same_origin_accepted_in_standalone() {
        let h = headers(Some("http://127.0.0.1:7681"), Some("127.0.0.1:7681"));
        assert_eq!(validate_origin(&h, &[], true), OriginDecision::Accept);
    }

    #[test]
    fn same_origin_rejected_in_trust_proxy() {
        let h = headers(Some("http://127.0.0.1:7681"), Some("127.0.0.1:7681"));
        assert!(matches!(
            validate_origin(&h, &[], false),
            OriginDecision::Reject {
                reason: OriginReject::NotAllowed,
                ..
            }
        ));
    }

    #[test]
    fn sibling_subdomain_rejected() {
        let allowed = vec![normalize_origin("https://app.example.com").unwrap()];
        let h = headers(Some("https://evil.example.com"), Some("localhost:7681"));
        assert!(matches!(
            validate_origin(&h, &allowed, false),
            OriginDecision::Reject {
                reason: OriginReject::NotAllowed,
                ..
            }
        ));
    }

    #[test]
    fn default_port_normalization_in_allowlist() {
        let allowed = vec![normalize_origin("https://host").unwrap()];
        let h = headers(Some("https://host:443"), Some("localhost:7681"));
        assert_eq!(validate_origin(&h, &allowed, false), OriginDecision::Accept);
    }
}
