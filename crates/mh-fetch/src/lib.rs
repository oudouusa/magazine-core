//! Safe host fetch broker for plugin `fetch_request` traffic.
//!
//! This crate implements generic network safety only. It intentionally does
//! not contain site-specific cookies, proxy bypass, challenge handling, or
//! scraping policy.

use std::collections::BTreeMap;
use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, CONTENT_LENGTH, LOCATION};
use serde::{Deserialize, Serialize};
use url::Url;

/// Raw response body cap. Base64 expansion plus JSON envelope must remain below
/// the protocol's 8 MiB frame limit.
pub const DEFAULT_MAX_RESPONSE_BYTES: usize = 5 * 1024 * 1024;
pub const DEFAULT_MAX_REDIRECTS: usize = 10;

/// Plugin fetch request body.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct FetchRequest {
    pub url: String,
    pub method: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

/// Host fetch response body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FetchResponse {
    pub status: u16,
    pub final_url: String,
    pub body_base64: String,
}

/// Safe fetch policy supplied by the host from the plugin manifest and runtime.
#[derive(Debug, Clone)]
pub struct FetchPolicy {
    pub allowed_domains: Vec<String>,
    pub connect_timeout: Duration,
    pub total_timeout: Duration,
    pub max_response_bytes: usize,
    pub max_redirects: usize,
}

impl FetchPolicy {
    pub fn for_allowed_domains(allowed_domains: Vec<String>) -> Self {
        Self {
            allowed_domains,
            connect_timeout: Duration::from_secs(10),
            total_timeout: Duration::from_secs(30),
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
            max_redirects: DEFAULT_MAX_REDIRECTS,
        }
    }
}

impl Default for FetchPolicy {
    fn default() -> Self {
        Self::for_allowed_domains(Vec::new())
    }
}

/// DNS resolver seam used by tests and production.
trait Resolver {
    fn resolve(
        &self,
        host: &str,
        port: u16,
        timeout: Duration,
    ) -> Result<Vec<SocketAddr>, FetchError>;
}

#[derive(Debug, Clone, Copy, Default)]
struct SystemResolver;

impl Resolver for SystemResolver {
    fn resolve(
        &self,
        host: &str,
        port: u16,
        timeout: Duration,
    ) -> Result<Vec<SocketAddr>, FetchError> {
        let host = host.to_string();
        resolve_with_timeout(timeout, move || {
            (host.as_str(), port)
                .to_socket_addrs()
                .map(|addrs| addrs.collect::<Vec<_>>())
                .map_err(|err| FetchError::Network(format!("DNS resolution failed: {err}")))
        })
    }
}

fn resolve_with_timeout(
    timeout: Duration,
    resolve: impl FnOnce() -> Result<Vec<SocketAddr>, FetchError> + Send + 'static,
) -> Result<Vec<SocketAddr>, FetchError> {
    let (tx, rx) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let _ = tx.send(resolve());
    });
    let addrs = rx.recv_timeout(timeout).map_err(|err| match err {
        mpsc::RecvTimeoutError::Timeout => FetchError::Timeout,
        mpsc::RecvTimeoutError::Disconnected => {
            FetchError::Network("DNS resolver thread exited".to_string())
        }
    })??;
    if addrs.is_empty() {
        return Err(FetchError::Policy("DNS returned no addresses".to_string()));
    }
    Ok(addrs)
}

/// HTTP transport seam. Production uses reqwest with redirects and proxies off.
trait Transport {
    fn send(&self, request: TransportRequest<'_>) -> Result<TransportResponse, FetchError>;
}

struct TransportRequest<'a> {
    url: &'a Url,
    method: FetchMethod,
    headers: &'a BTreeMap<String, String>,
    resolved_addrs: &'a [SocketAddr],
    connect_timeout: Duration,
    timeout: Duration,
    max_response_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TransportResponse {
    status: u16,
    location: Option<String>,
    body: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FetchMethod {
    Get,
    Head,
}

impl FetchMethod {
    fn parse(raw: &str) -> Result<Self, FetchError> {
        match raw {
            method if method.eq_ignore_ascii_case("GET") => Ok(Self::Get),
            method if method.eq_ignore_ascii_case("HEAD") => Ok(Self::Head),
            _ => Err(FetchError::Policy(format!(
                "unsupported fetch method {raw:?}; only GET and HEAD are allowed"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct ReqwestTransport;

impl Transport for ReqwestTransport {
    fn send(&self, request: TransportRequest<'_>) -> Result<TransportResponse, FetchError> {
        let host = request
            .url
            .host_str()
            .ok_or_else(|| FetchError::Policy("fetch URL missing host".to_string()))?;
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(request.connect_timeout)
            .timeout(request.timeout)
            .resolve_to_addrs(host, request.resolved_addrs)
            .build()
            .map_err(|err| FetchError::Network(format!("failed to build HTTP client: {err}")))?;

        let method = match request.method {
            FetchMethod::Get => reqwest::Method::GET,
            FetchMethod::Head => reqwest::Method::HEAD,
        };
        let mut headers = HeaderMap::new();
        for (name, value) in request.headers {
            let header_name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| FetchError::Policy(format!("invalid request header name {name:?}")))?;
            let header_value = HeaderValue::from_str(value).map_err(|_| {
                FetchError::Policy(format!("invalid request header value for {name:?}"))
            })?;
            headers.insert(header_name, header_value);
        }

        let response = client
            .request(method, request.url.clone())
            .headers(headers)
            .send()
            .map_err(|err| FetchError::Network(format!("HTTP request failed: {err}")))?;
        let status = response.status().as_u16();
        let location = response
            .headers()
            .get(LOCATION.as_str())
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        if let Some(length) = response
            .headers()
            .get(CONTENT_LENGTH.as_str())
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<usize>().ok())
        {
            if length > request.max_response_bytes {
                return Err(FetchError::ResponseTooLarge {
                    limit: request.max_response_bytes,
                });
            }
        }
        let mut body = Vec::new();
        let mut limited = response.take((request.max_response_bytes as u64).saturating_add(1));
        limited
            .read_to_end(&mut body)
            .map_err(|err| FetchError::Network(format!("failed to read response body: {err}")))?;
        if body.len() > request.max_response_bytes {
            return Err(FetchError::ResponseTooLarge {
                limit: request.max_response_bytes,
            });
        }
        Ok(TransportResponse {
            status,
            location,
            body,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct SafeFetcher {
    inner: SafeFetcherParts<SystemResolver, ReqwestTransport>,
}

impl SafeFetcher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn fetch(
        &self,
        request: FetchRequest,
        policy: &FetchPolicy,
    ) -> Result<FetchResponse, FetchError> {
        self.inner.fetch(request, policy)
    }
}

#[derive(Debug, Clone, Default)]
struct SafeFetcherParts<R, T> {
    resolver: R,
    transport: T,
}

impl<R, T> SafeFetcherParts<R, T> {
    #[cfg(test)]
    fn with_parts(resolver: R, transport: T) -> Self {
        Self {
            resolver,
            transport,
        }
    }
}

impl<R: Resolver, T: Transport> SafeFetcherParts<R, T> {
    fn fetch(
        &self,
        request: FetchRequest,
        policy: &FetchPolicy,
    ) -> Result<FetchResponse, FetchError> {
        let method = FetchMethod::parse(&request.method)?;
        validate_headers(&request.headers)?;
        let mut url = Url::parse(&request.url)
            .map_err(|err| FetchError::Policy(format!("invalid fetch URL: {err}")))?;
        let started = Instant::now();

        for redirect_count in 0..=policy.max_redirects {
            let remaining = remaining_timeout(started, policy.total_timeout)?;
            validate_url(&url, policy)?;
            let host = url
                .host_str()
                .ok_or_else(|| FetchError::Policy("fetch URL missing host".to_string()))?;
            let port = url
                .port_or_known_default()
                .ok_or_else(|| FetchError::Policy("fetch URL missing port".to_string()))?;
            let dns_timeout = policy.connect_timeout.min(remaining);
            let addrs = self.resolver.resolve(host, port, dns_timeout)?;
            validate_resolved_addrs(&addrs)?;
            let remaining = remaining_timeout(started, policy.total_timeout)?;
            let response = self.transport.send(TransportRequest {
                url: &url,
                method,
                headers: &request.headers,
                resolved_addrs: &addrs,
                connect_timeout: policy.connect_timeout.min(remaining),
                timeout: remaining,
                max_response_bytes: policy.max_response_bytes,
            })?;
            if response.body.len() > policy.max_response_bytes {
                return Err(FetchError::ResponseTooLarge {
                    limit: policy.max_response_bytes,
                });
            }

            if is_redirect(response.status) {
                if redirect_count == policy.max_redirects {
                    return Err(FetchError::TooManyRedirects {
                        limit: policy.max_redirects,
                    });
                }
                let location = response.location.ok_or_else(|| {
                    FetchError::Policy("redirect response missing Location".to_string())
                })?;
                url = url
                    .join(&location)
                    .map_err(|err| FetchError::Policy(format!("invalid redirect URL: {err}")))?;
                continue;
            }

            return Ok(FetchResponse {
                status: response.status,
                final_url: url.to_string(),
                body_base64: base64::engine::general_purpose::STANDARD.encode(response.body),
            });
        }

        Err(FetchError::TooManyRedirects {
            limit: policy.max_redirects,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    Network(String),
    Policy(String),
    ResponseTooLarge { limit: usize },
    Timeout,
    TooManyRedirects { limit: usize },
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::Network(err) => write!(f, "network error: {err}"),
            FetchError::Policy(err) => write!(f, "fetch policy error: {err}"),
            FetchError::ResponseTooLarge { limit } => {
                write!(f, "response body exceeded limit {limit} bytes")
            }
            FetchError::Timeout => write!(f, "fetch timed out"),
            FetchError::TooManyRedirects { limit } => {
                write!(f, "too many redirects; limit is {limit}")
            }
        }
    }
}

impl std::error::Error for FetchError {}

fn validate_url(url: &Url, policy: &FetchPolicy) -> Result<(), FetchError> {
    match url.scheme() {
        "http" | "https" => {}
        scheme => {
            return Err(FetchError::Policy(format!(
                "unsupported URL scheme {scheme:?}; only http and https are allowed"
            )));
        }
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(FetchError::Policy(
            "fetch URL userinfo is not allowed".to_string(),
        ));
    }
    let host = url
        .host_str()
        .ok_or_else(|| FetchError::Policy("fetch URL missing host".to_string()))?;
    if !domain_allowed(host, &policy.allowed_domains) {
        return Err(FetchError::Policy(format!(
            "host {host:?} is not in manifest allowed_domains"
        )));
    }
    Ok(())
}

fn validate_headers(headers: &BTreeMap<String, String>) -> Result<(), FetchError> {
    for (name, value) in headers {
        HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| FetchError::Policy(format!("invalid request header name {name:?}")))?;
        HeaderValue::from_str(value).map_err(|_| {
            FetchError::Policy(format!("invalid request header value for {name:?}"))
        })?;
        if is_disallowed_header(name) {
            return Err(FetchError::Policy(format!(
                "request header {name:?} is not allowed"
            )));
        }
    }
    Ok(())
}

fn validate_resolved_addrs(addrs: &[SocketAddr]) -> Result<(), FetchError> {
    if addrs.is_empty() {
        return Err(FetchError::Policy("DNS returned no addresses".to_string()));
    }
    for addr in addrs {
        if forbidden_ip(addr.ip()) {
            return Err(FetchError::Policy(format!(
                "resolved IP {} is not allowed",
                addr.ip()
            )));
        }
    }
    Ok(())
}

fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

fn remaining_timeout(started: Instant, total: Duration) -> Result<Duration, FetchError> {
    total
        .checked_sub(started.elapsed())
        .ok_or(FetchError::Timeout)
}

fn domain_allowed(host: &str, allowed_domains: &[String]) -> bool {
    let host = normalize_domain(host);
    allowed_domains.iter().any(|allowed| {
        let allowed = normalize_domain(allowed);
        !allowed.is_empty() && (host == allowed || host.ends_with(&format!(".{allowed}")))
    })
}

fn normalize_domain(value: &str) -> String {
    value.trim().trim_end_matches('.').to_ascii_lowercase()
}

fn is_disallowed_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "authorization"
            | "cookie"
            | "proxy-authorization"
            | "set-cookie"
            | "host"
            | "connection"
            | "transfer-encoding"
            | "content-length"
    )
}

fn forbidden_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || ip.is_multicast()
                || shared_carrier_grade_nat(ip)
        }
        IpAddr::V6(ip) => {
            if let Some(v4) = ip.to_ipv4_mapped() {
                return forbidden_ip(IpAddr::V4(v4));
            }
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
        }
    }
}

fn shared_carrier_grade_nat(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 100 && (64..=127).contains(&octets[1])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::net::{IpAddr, Ipv4Addr};

    #[derive(Clone)]
    struct FakeResolver {
        addrs: HashMap<String, Vec<SocketAddr>>,
    }

    impl FakeResolver {
        fn new(entries: &[(&str, IpAddr)]) -> Self {
            let addrs = entries
                .iter()
                .map(|(host, ip)| ((*host).to_string(), vec![SocketAddr::new(*ip, 80)]))
                .collect();
            Self { addrs }
        }
    }

    impl Resolver for FakeResolver {
        fn resolve(
            &self,
            host: &str,
            _port: u16,
            _timeout: Duration,
        ) -> Result<Vec<SocketAddr>, FetchError> {
            self.addrs
                .get(host)
                .cloned()
                .ok_or_else(|| FetchError::Network(format!("missing fake DNS for {host}")))
        }
    }

    #[derive(Clone)]
    struct FakeTransport {
        responses: HashMap<String, TransportResponse>,
    }

    impl FakeTransport {
        fn new(entries: &[(&str, TransportResponse)]) -> Self {
            Self {
                responses: entries
                    .iter()
                    .map(|(url, response)| ((*url).to_string(), response.clone()))
                    .collect(),
            }
        }
    }

    impl Transport for FakeTransport {
        fn send(&self, request: TransportRequest<'_>) -> Result<TransportResponse, FetchError> {
            assert!(!request.resolved_addrs.is_empty());
            self.responses
                .get(request.url.as_str())
                .cloned()
                .ok_or_else(|| {
                    FetchError::Network(format!("missing fake HTTP for {}", request.url))
                })
        }
    }

    fn policy(domains: &[&str]) -> FetchPolicy {
        FetchPolicy {
            allowed_domains: domains.iter().map(|domain| (*domain).to_string()).collect(),
            connect_timeout: Duration::from_secs(1),
            total_timeout: Duration::from_secs(5),
            max_response_bytes: 8,
            max_redirects: 3,
        }
    }

    fn public_ip() -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))
    }

    #[test]
    fn resolver_timeout_returns_before_blocking_lookup_finishes() {
        let started = Instant::now();
        let err = resolve_with_timeout(Duration::from_millis(5), || {
            thread::sleep(Duration::from_millis(100));
            Ok(vec![SocketAddr::new(public_ip(), 80)])
        })
        .unwrap_err();

        assert_eq!(err, FetchError::Timeout);
        assert!(started.elapsed() < Duration::from_millis(80));
    }

    #[test]
    fn fetch_success_encodes_body_and_final_url() {
        let fetcher = SafeFetcherParts::with_parts(
            FakeResolver::new(&[("example.test", public_ip())]),
            FakeTransport::new(&[(
                "https://example.test/page",
                TransportResponse {
                    status: 200,
                    location: None,
                    body: b"ok".to_vec(),
                },
            )]),
        );

        let response = fetcher
            .fetch(
                FetchRequest {
                    url: "https://example.test/page".to_string(),
                    method: "GET".to_string(),
                    headers: BTreeMap::new(),
                },
                &policy(&["example.test"]),
            )
            .unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(response.final_url, "https://example.test/page");
        assert_eq!(response.body_base64, "b2s=");
    }

    #[test]
    fn allowed_domains_accept_exact_and_subdomain_only() {
        assert!(domain_allowed(
            "example.test",
            &["example.test".to_string()]
        ));
        assert!(domain_allowed(
            "cdn.example.test",
            &["example.test".to_string()]
        ));
        assert!(!domain_allowed(
            "badexample.test",
            &["example.test".to_string()]
        ));
    }

    #[test]
    fn rejects_unsupported_scheme_method_and_credential_headers() {
        let fetcher = SafeFetcherParts::with_parts(
            FakeResolver::new(&[("example.test", public_ip())]),
            FakeTransport::new(&[]),
        );
        assert!(matches!(
            fetcher.fetch(
                FetchRequest {
                    url: "file:///etc/passwd".to_string(),
                    method: "GET".to_string(),
                    headers: BTreeMap::new(),
                },
                &policy(&["example.test"]),
            ),
            Err(FetchError::Policy(_))
        ));
        assert!(matches!(
            fetcher.fetch(
                FetchRequest {
                    url: "https://example.test@evil.test/".to_string(),
                    method: "GET".to_string(),
                    headers: BTreeMap::new(),
                },
                &policy(&["evil.test"]),
            ),
            Err(FetchError::Policy(message)) if message.contains("userinfo")
        ));
        assert!(matches!(
            FetchMethod::parse("POST"),
            Err(FetchError::Policy(_))
        ));
        assert!(matches!(
            validate_headers(&BTreeMap::from([(
                "Authorization".to_string(),
                "secret".to_string()
            )])),
            Err(FetchError::Policy(message)) if !message.contains("secret")
        ));
        assert!(matches!(
            validate_headers(&BTreeMap::from([(
                "Host".to_string(),
                "example.test".to_string()
            )])),
            Err(FetchError::Policy(_))
        ));
    }

    #[test]
    fn rejects_private_loopback_link_local_and_carrier_nat_addresses() {
        for ip in [
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            IpAddr::V4(Ipv4Addr::new(169, 254, 1, 1)),
            IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1)),
            "::1".parse().unwrap(),
            "::ffff:127.0.0.1".parse().unwrap(),
            "::ffff:10.0.0.1".parse().unwrap(),
            "::ffff:169.254.1.1".parse().unwrap(),
            "::ffff:100.64.0.1".parse().unwrap(),
            "fc00::1".parse().unwrap(),
            "fe80::1".parse().unwrap(),
        ] {
            assert!(forbidden_ip(ip), "{ip} should be forbidden");
        }
        assert!(!forbidden_ip(public_ip()));
    }

    #[test]
    fn redirects_are_revalidated() {
        let fetcher = SafeFetcherParts::with_parts(
            FakeResolver::new(&[("example.test", public_ip()), ("evil.test", public_ip())]),
            FakeTransport::new(&[(
                "https://example.test/start",
                TransportResponse {
                    status: 302,
                    location: Some("https://evil.test/final".to_string()),
                    body: Vec::new(),
                },
            )]),
        );

        let err = fetcher
            .fetch(
                FetchRequest {
                    url: "https://example.test/start".to_string(),
                    method: "GET".to_string(),
                    headers: BTreeMap::new(),
                },
                &policy(&["example.test"]),
            )
            .unwrap_err();

        assert!(err.to_string().contains("allowed_domains"));
    }

    #[test]
    fn redirect_to_private_ip_is_rejected_after_dns_resolution() {
        let fetcher = SafeFetcherParts::with_parts(
            FakeResolver::new(&[
                ("example.test", public_ip()),
                (
                    "internal.example.test",
                    IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                ),
            ]),
            FakeTransport::new(&[(
                "https://example.test/start",
                TransportResponse {
                    status: 302,
                    location: Some("https://internal.example.test/final".to_string()),
                    body: Vec::new(),
                },
            )]),
        );

        let err = fetcher
            .fetch(
                FetchRequest {
                    url: "https://example.test/start".to_string(),
                    method: "GET".to_string(),
                    headers: BTreeMap::new(),
                },
                &policy(&["example.test"]),
            )
            .unwrap_err();

        assert!(err.to_string().contains("resolved IP"));
    }

    #[test]
    fn localhost_allowlist_does_not_override_ip_rejection() {
        let fetcher = SafeFetcherParts::with_parts(
            FakeResolver::new(&[("localhost", IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)))]),
            FakeTransport::new(&[]),
        );

        let err = fetcher
            .fetch(
                FetchRequest {
                    url: "http://localhost/page".to_string(),
                    method: "GET".to_string(),
                    headers: BTreeMap::new(),
                },
                &policy(&["localhost"]),
            )
            .unwrap_err();

        assert!(err.to_string().contains("resolved IP"));
    }

    #[test]
    fn response_size_cap_is_enforced_by_transport_contract() {
        let response = TransportResponse {
            status: 200,
            location: None,
            body: b"too-large".to_vec(),
        };
        let fetcher = SafeFetcherParts::with_parts(
            FakeResolver::new(&[("example.test", public_ip())]),
            FakeTransport::new(&[("https://example.test/page", response)]),
        );

        let err = fetcher
            .fetch(
                FetchRequest {
                    url: "https://example.test/page".to_string(),
                    method: "GET".to_string(),
                    headers: BTreeMap::new(),
                },
                &policy(&["example.test"]),
            )
            .unwrap_err();

        assert!(matches!(err, FetchError::ResponseTooLarge { limit: 8 }));
    }
}
