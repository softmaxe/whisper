//! OpenAI-compatible base URL handling and the HTTPS requirement.

use std::net::Ipv4Addr;

use url::{Host, Url};

/// Split off query and hash so suffix stripping and joins touch the path
/// only: gateways may put `?api-version=…` on a pasted completions URL.
fn split_url_decorators(value: &str) -> (&str, &str, &str) {
    let (rest, hash) = match value.find('#') {
        Some(i) => value.split_at(i),
        None => (value, ""),
    };
    let (path, query) = match rest.find('?') {
        Some(i) => rest.split_at(i),
        None => (rest, ""),
    };
    (path, query, hash)
}

fn strip_suffix_ignore_ascii_case<'a>(value: &'a str, suffix: &str) -> Option<&'a str> {
    let cut = value.len().checked_sub(suffix.len())?;
    let tail = value.get(cut..)?;
    tail.eq_ignore_ascii_case(suffix).then(|| &value[..cut])
}

const SUFFIX_REPLACEMENTS: [(&str, &str); 10] = [
    ("/v1/chat/completions", "/v1"),
    ("/chat/completions", ""),
    ("/v1/responses", "/v1"),
    ("/responses", ""),
    ("/v1/models", "/v1"),
    ("/models", ""),
    ("/v1/audio/transcriptions", "/v1"),
    ("/audio/transcriptions", ""),
    ("/v1/audio/translations", "/v1"),
    ("/audio/translations", ""),
];

/// Reduce a pasted endpoint URL to its API base: trimmed, known endpoint
/// suffixes removed (case-insensitively, canonicalizing to "/v1"), trailing
/// slashes dropped, query and hash kept.
pub fn normalize_base_url(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let (raw_path, query, hash) = split_url_decorators(trimmed);
    let mut normalized = raw_path.to_string();
    for (suffix, replacement) in SUFFIX_REPLACEMENTS {
        if let Some(head) = strip_suffix_ignore_ascii_case(&normalized, suffix) {
            normalized = format!("{head}{replacement}")
                .trim_end_matches('/')
                .to_string();
        }
    }
    format!("{}{query}{hash}", normalized.trim_end_matches('/'))
}

/// Join a base and an endpoint path, inserting the path before any query or
/// hash. `None` when the base is empty.
pub fn build_api_url(base: &str, path: &str) -> Option<String> {
    let normalized_base = normalize_base_url(base);
    if normalized_base.is_empty() {
        return None;
    }
    if path.is_empty() {
        return Some(normalized_base);
    }
    let slash = if path.starts_with('/') { "" } else { "/" };
    let (origin_and_path, query, hash) = split_url_decorators(&normalized_base);
    Some(format!("{origin_and_path}{slash}{path}{query}{hash}"))
}

/// Append "/v1" to the base's path unless it already ends with it.
pub fn ensure_v1_suffix(base: &str) -> String {
    if base.is_empty() {
        return String::new();
    }
    let normalized = normalize_base_url(base);
    let normalized = if normalized.is_empty() {
        base.to_string()
    } else {
        normalized
    };
    let (path, query, hash) = split_url_decorators(&normalized);
    let suffix = if path.ends_with("/v1") { "" } else { "/v1" };
    format!("{path}{suffix}{query}{hash}")
}

/// Ordered bases to try when listing models. Self-hosted servers serve the
/// API under /v1 even when users enter the bare origin, and LM Studio's native
/// REST base (/api/v1, /api/v0) has its OpenAI-compatible sibling at /v1.
pub fn model_list_base_candidates(base: &str) -> Vec<String> {
    let normalized = normalize_base_url(base);
    if normalized.is_empty() {
        return Vec::new();
    }
    let (path, query, hash) = split_url_decorators(&normalized);
    // `^(.+?)\/api\/v[01]$` with /i: a non-empty prefix before the native base.
    for native in ["/api/v0", "/api/v1"] {
        if let Some(prefix) = strip_suffix_ignore_ascii_case(path, native)
            && !prefix.is_empty()
        {
            let sibling = format!("{prefix}/v1{query}{hash}");
            return vec![normalized.clone(), sibling];
        }
    }
    let with_v1 = ensure_v1_suffix(&normalized);
    if with_v1 == normalized {
        vec![normalized]
    } else {
        vec![normalized, with_v1]
    }
}

/// A real dotted quad, so names like "127.example.com" cannot pass the
/// private-range checks by prefix.
fn parse_ipv4_literal(hostname: &str) -> Option<[u8; 4]> {
    let parts: Vec<&str> = hostname.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    let mut octets = [0u8; 4];
    for (slot, part) in octets.iter_mut().zip(parts) {
        // Reject empty labels, leading zeros and non-decimal forms.
        let valid = matches!(part.len(), 1..=3)
            && part.bytes().all(|b| b.is_ascii_digit())
            && (part == "0" || !part.starts_with('0'));
        if !valid {
            return None;
        }
        *slot = part.parse().ok()?;
    }
    Some(octets)
}

fn is_private_host(hostname: &str) -> bool {
    let lower = hostname.to_lowercase();
    let h = lower.strip_prefix('[').unwrap_or(&lower);
    let h = h.strip_suffix(']').unwrap_or(h);

    if matches!(h, "localhost" | "0.0.0.0" | "::1") {
        return true;
    }
    if let Some([a, b, _, _]) = parse_ipv4_literal(h)
        && (a == 127
            || a == 10
            || (a == 192 && b == 168)
            || (a == 172 && (16..=31).contains(&b))
            // RFC 6598 CGNAT (Tailscale): 100.64.0.0/10
            || (a == 100 && (64..=127).contains(&b))
            || (a == 169 && b == 254))
    {
        return true;
    }
    // Link-local is fe80::/10 (fe80–febf); unique local is fc00::/7.
    let is_ipv6 = h.contains(':');
    if is_ipv6
        && (["fe8", "fe9", "fea", "feb", "fc", "fd"]
            .iter()
            .any(|p| h.starts_with(p)))
    {
        return true;
    }
    // .local (mDNS) and Tailscale MagicDNS names stay inside the user's network.
    h.ends_with(".local") || h.ends_with(".ts.net")
}

/// WHATWG-style hostname: IPv4 in dotted form (after expanding shorthands
/// like "127.1"), IPv6 bracketed, domains lowercased.
fn whatwg_hostname(url: &Url) -> Option<String> {
    Some(match url.host()? {
        Host::Domain(d) => d.to_lowercase(),
        Host::Ipv4(addr) => Ipv4Addr::to_string(&addr),
        Host::Ipv6(addr) => format!("[{addr}]"),
    })
}

/// HTTPS anywhere, or plain HTTP to a loopback/private/link-local host.
pub fn is_secure_http_endpoint(url: &str) -> bool {
    let Ok(parsed) = Url::parse(url) else {
        return false;
    };
    match parsed.scheme() {
        "https" => true,
        "http" => whatwg_hostname(&parsed).is_some_and(|h| is_private_host(&h)),
        _ => false,
    }
}

/// What is wrong with an endpoint base, worst first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndpointIssue {
    /// Not an http(s) URL.
    Invalid,
    /// Plain HTTP to a public host.
    NeedsHttps,
    /// The path does not end in /v1, where most servers serve the API.
    NoV1,
}

/// The URL a request to `path` will hit for a typed base, plus the first
/// problem with that base. `None` while the input is blank.
pub fn endpoint_check(raw: &str, path: &str) -> Option<(String, Option<EndpointIssue>)> {
    let base = normalize_base_url(raw);
    if base.is_empty() {
        return None;
    }
    let target = build_api_url(&base, path).unwrap_or_default();
    let valid = Url::parse(&base).is_ok_and(|u| matches!(u.scheme(), "http" | "https"));
    let (base_path, _, _) = split_url_decorators(&base);
    let issue = if !valid {
        Some(EndpointIssue::Invalid)
    } else if !is_secure_http_endpoint(&base) {
        Some(EndpointIssue::NeedsHttps)
    } else if !base_path.ends_with("/v1") {
        Some(EndpointIssue::NoV1)
    } else {
        None
    };
    Some((target, issue))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_check_resolves_and_flags() {
        assert_eq!(endpoint_check("  ", "/chat/completions"), None);
        assert_eq!(
            endpoint_check(
                "http://100.101.102.103:11434/v1/chat/completions",
                "/chat/completions"
            ),
            Some((
                "http://100.101.102.103:11434/v1/chat/completions".into(),
                None
            ))
        );
        assert_eq!(
            endpoint_check("http://localhost:8000", "/audio/transcriptions")
                .unwrap()
                .1,
            Some(EndpointIssue::NoV1)
        );
        assert_eq!(
            endpoint_check("http://api.example.com/v1", "/chat/completions")
                .unwrap()
                .1,
            Some(EndpointIssue::NeedsHttps)
        );
        assert_eq!(
            endpoint_check("localhost:8000/v1", "/models").unwrap().1,
            Some(EndpointIssue::Invalid)
        );
    }

    #[test]
    fn empty_bases() {
        assert_eq!(normalize_base_url(""), "");
        assert_eq!(normalize_base_url("   "), "");
    }

    #[test]
    fn trailing_slashes() {
        assert_eq!(
            normalize_base_url("https://api.example.com/"),
            "https://api.example.com"
        );
        assert_eq!(
            normalize_base_url("https://api.example.com///"),
            "https://api.example.com"
        );
    }

    #[test]
    fn endpoint_suffixes_reduced() {
        let cases = [
            (
                "https://api.example.com/v1/chat/completions",
                "https://api.example.com/v1",
            ),
            (
                "https://api.example.com/chat/completions",
                "https://api.example.com",
            ),
            (
                "https://api.example.com/v1/responses",
                "https://api.example.com/v1",
            ),
            (
                "https://api.example.com/v1/models",
                "https://api.example.com/v1",
            ),
            (
                "https://api.example.com/v1/audio/transcriptions",
                "https://api.example.com/v1",
            ),
            (
                "https://api.example.com/audio/transcriptions",
                "https://api.example.com",
            ),
            (
                "https://api.example.com/v1/audio/translations",
                "https://api.example.com/v1",
            ),
            ("https://api.openai.com/v1", "https://api.openai.com/v1"),
            (
                "https://api.example.com/V1/Chat/Completions",
                "https://api.example.com/v1",
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(normalize_base_url(input), expected, "{input}");
        }
    }

    #[test]
    fn build_urls() {
        assert_eq!(
            build_api_url("https://api.openai.com/v1", "/responses").as_deref(),
            Some("https://api.openai.com/v1/responses")
        );
        assert_eq!(
            build_api_url("https://api.openai.com/v1", "responses").as_deref(),
            Some("https://api.openai.com/v1/responses")
        );
        assert_eq!(build_api_url("", "/chat/completions"), None);
        assert_eq!(build_api_url("   ", "/chat/completions"), None);
        assert_eq!(
            build_api_url("https://api.example.com/v1/chat/completions", "/responses").as_deref(),
            Some("https://api.example.com/v1/responses")
        );
        assert_eq!(
            build_api_url("https://api.openai.com/v1", "").as_deref(),
            Some("https://api.openai.com/v1")
        );
    }

    #[test]
    fn query_and_hash_preserved() {
        assert_eq!(
            normalize_base_url(
                "https://api.example.com/v1/chat/completions?api-version=2024-02-01"
            ),
            "https://api.example.com/v1?api-version=2024-02-01"
        );
        assert_eq!(
            normalize_base_url("https://api.example.com/v1/models?api-key=secret#section"),
            "https://api.example.com/v1?api-key=secret#section"
        );
        assert_eq!(
            build_api_url(
                "https://api.example.com/v1/chat/completions?api-version=2024-02-01",
                "/responses"
            )
            .as_deref(),
            Some("https://api.example.com/v1/responses?api-version=2024-02-01")
        );
        assert_eq!(
            build_api_url("https://gateway.example.com/v1?api-key=secret", "/models").as_deref(),
            Some("https://gateway.example.com/v1/models?api-key=secret")
        );
        assert_eq!(
            build_api_url(
                "https://gateway.example.com/v1?api-key=secret#frag",
                "chat/completions"
            )
            .as_deref(),
            Some("https://gateway.example.com/v1/chat/completions?api-key=secret#frag")
        );
        assert_eq!(
            ensure_v1_suffix("https://gateway.example.com/v1?api-key=secret"),
            "https://gateway.example.com/v1?api-key=secret"
        );
        assert_eq!(
            ensure_v1_suffix("https://gateway.example.com?api-key=secret"),
            "https://gateway.example.com/v1?api-key=secret"
        );
    }

    #[test]
    fn model_list_candidates() {
        let c = model_list_base_candidates;
        assert_eq!(
            c("http://127.0.0.1:1234"),
            ["http://127.0.0.1:1234", "http://127.0.0.1:1234/v1"]
        );
        assert_eq!(
            c("http://127.0.0.1:1234/api/v1"),
            ["http://127.0.0.1:1234/api/v1", "http://127.0.0.1:1234/v1"]
        );
        assert_eq!(
            c("http://127.0.0.1:1234/api/v0"),
            ["http://127.0.0.1:1234/api/v0", "http://127.0.0.1:1234/v1"]
        );
        assert_eq!(c("http://127.0.0.1:1234/v1"), ["http://127.0.0.1:1234/v1"]);
        assert_eq!(
            c("https://api.together.xyz/v1"),
            ["https://api.together.xyz/v1"]
        );
        assert_eq!(
            c("https://openrouter.ai/api/v1"),
            ["https://openrouter.ai/api/v1", "https://openrouter.ai/v1"]
        );
        assert_eq!(
            c("http://127.0.0.1:1234/"),
            ["http://127.0.0.1:1234", "http://127.0.0.1:1234/v1"]
        );
        assert_eq!(
            c("http://127.0.0.1:1234/v1/models"),
            ["http://127.0.0.1:1234/v1"]
        );
        assert!(c("").is_empty());
        assert_eq!(
            c("https://gateway.example.com/v1?api-key=secret"),
            ["https://gateway.example.com/v1?api-key=secret"]
        );
        assert_eq!(
            c("https://gateway.example.com?api-key=secret"),
            [
                "https://gateway.example.com?api-key=secret",
                "https://gateway.example.com/v1?api-key=secret"
            ]
        );
        assert_eq!(
            c("http://127.0.0.1:1234/api/v1?token=abc"),
            [
                "http://127.0.0.1:1234/api/v1?token=abc",
                "http://127.0.0.1:1234/v1?token=abc"
            ]
        );
        assert_eq!(
            c("https://api.example.com/v1/models?api-version=2024-02-01"),
            ["https://api.example.com/v1?api-version=2024-02-01"]
        );
    }

    #[test]
    fn secure_endpoints() {
        let secure = [
            "https://api.openai.com/v1",
            "http://localhost:8080",
            "http://127.0.0.1:5000",
            "http://127.0.1.1:3000",
            "http://0.0.0.0:8000",
            "http://[::1]:8080",
            "https://127.example.com/v1",
            "http://127.1/v1",
            "http://127.0.1/v1",
            "http://10.0.0.5:8080",
            "http://192.168.1.100:8080",
            "http://172.16.0.1:8080",
            "http://172.31.255.255:8080",
            "http://100.64.0.1:8080",
            "http://100.127.255.254:8080",
            "http://169.254.1.1:8080",
            "http://[fe80::1]:8080",
            "http://[fe90::1]:8080",
            "http://[fea0::1]:8080",
            "http://[febf::1]:8080",
            "http://[fc00::1]:8080",
            "http://[fd12::1]:8080",
            "http://myserver.local:8080",
            "http://MyServer.LOCAL:8080",
            "http://myserver.tailnet-1234.ts.net:8080",
            "http://192.168.1.20:5001/v1",
        ];
        for url in secure {
            assert!(is_secure_http_endpoint(url), "{url}");
        }
        let insecure = [
            "http://api.openai.com/v1",
            "http://127.example.com/v1",
            "http://10.example.com/v1",
            "http://192.168.example.com/v1",
            "http://172.16.example.com/v1",
            "http://100.64.example.com/v1",
            "http://169.254.example.com/v1",
            "http://127.0.0.1.nip.io/v1",
            "http://172.15.0.1:8080",
            "http://172.32.0.1:8080",
            "http://100.63.0.1:8080",
            "http://100.128.0.1:8080",
            "http://[fe7f::1]:8080",
            "http://myts.net:8080",
            "http://evil-ts.net:8080",
            "http://ts.net:8080",
            "",
            "not-a-url",
            "http://public.example.com/v1",
            "ftp://192.168.1.20/v1",
            "ws://127.0.0.1:5001/v1",
        ];
        for url in insecure {
            assert!(!is_secure_http_endpoint(url), "{url}");
        }
    }
}
