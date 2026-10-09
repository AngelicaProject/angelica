//! HTTPS requests of plugins (the `http` interface): GET only, to the hosts
//! the manifest lists and the player approved, to public addresses only,
//! within the host's size and time limits. The resolver enforces the hosts
//! and addresses, so redirects are held to the same rules as the first
//! request. TLS is the operating system's (`native-tls`: `SChannel` and the
//! system's certificate store on Windows).
#![forbid(unsafe_code)]
use crate::error::{Error, Kind, Result};
use std::{
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr},
    sync::{Arc, OnceLock},
    time::Duration,
};
use ureq::{
    config::Config,
    http::Uri,
    unversioned::{
        resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver},
        transport::{
            Buffers, ConnectionDetails, Connector, LazyBuffers, NextTimeout, TcpConnector,
            Transport, TransportAdapter,
        },
    },
};

/// Bytes of a body `get` returns.
pub const MAX_BODY: u64 = 16 * 1024 * 1024;
/// Bytes of a body `download` writes.
pub const MAX_DOWNLOAD: u64 = 1024 * 1024 * 1024;
pub const MAX_REDIRECTS: u32 = 5;
/// Bytes of a URL.
pub const MAX_URL: usize = 4096;
/// Bytes of a request header's value.
pub const MAX_HEADER_VALUE: usize = 1024;
/// The request headers a plugin may set.
pub const REQUEST_HEADERS: [&str; 5] = [
    "accept",
    "accept-language",
    "if-none-match",
    "if-modified-since",
    "range",
];
pub const GET_TIME: Duration = Duration::from_secs(60);
pub const DOWNLOAD_TIME: Duration = Duration::from_mins(15);
const CONNECT_TIME: Duration = Duration::from_secs(15);
const RESPONSE_TIME: Duration = Duration::from_secs(30);

/// Whether `pattern` is a host name a manifest may list: lower-case ASCII
/// labels of letters, digits, and inner hyphens, at least two, not an
/// address. A leading `*.` matches every subdomain, not the name itself,
/// and needs two labels after it.
///
/// # Errors
/// Says what is wrong.
pub fn check_host_pattern(pattern: &str) -> std::result::Result<(), &'static str> {
    let name = pattern.strip_prefix("*.").unwrap_or(pattern);
    if name.len() > 253 {
        return Err("is longer than 253 bytes");
    }
    let labels: Vec<&str> = name.split('.').collect();
    if labels.len() < 2 {
        return Err("needs at least two labels, such as example.com");
    }
    for label in &labels {
        if label.is_empty()
            || label.len() > 63
            || label.starts_with('-')
            || label.ends_with('-')
            || !label
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(
                "has labels of 1 to 63 lower-case letters, digits, and inner hyphens; \
                 international names use their xn-- form",
            );
        }
    }
    if labels
        .last()
        .is_some_and(|last| last.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err("is an address, not a host name");
    }
    Ok(())
}

/// Whether one of `patterns` matches `host`.
fn listed(patterns: &[String], host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    patterns
        .iter()
        .any(|pattern| match pattern.strip_prefix("*.") {
            Some(suffix) => host
                .strip_suffix(suffix)
                .is_some_and(|rest| rest.len() > 1 && rest.ends_with('.')),
            None => host == *pattern,
        })
}

/// Whether `address` is reachable on the public internet: not loopback,
/// private, shared, link-local, multicast, broadcast, documentation,
/// benchmarking, reserved, or unspecified.
#[must_use]
pub fn public(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(v4) => public_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return public_v4(v4);
            }
            let segments = v6.segments();
            !(v6.is_unspecified()
                || v6.is_loopback()
                || v6.is_multicast()
                // Unique local fc00::/7 and link-local fe80::/10.
                || segments[0] & 0xFE00 == 0xFC00
                || segments[0] & 0xFFC0 == 0xFE80
                // Documentation 2001:db8::/32.
                || (segments[0] == 0x2001 && segments[1] == 0x0DB8)
                // IPv4-compatible and other addresses in ::/96, 64:ff9b::/96
                // translation, and 6to4 2002::/16, which embed IPv4.
                || segments[..6] == [0; 6]
                || segments[..6] == [0x64, 0xFF9B, 0, 0, 0, 0]
                || segments[0] == 0x2002)
        }
    }
}

fn public_v4(v4: Ipv4Addr) -> bool {
    let [a, b, c, _] = v4.octets();
    !(v4.is_unspecified()
        || v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_broadcast()
        || v4.is_documentation()
        || v4.is_multicast()
        || a == 0
        // Shared 100.64.0.0/10, IETF 192.0.0.0/24, benchmarking
        // 198.18.0.0/15, reserved 240.0.0.0/4.
        || (a == 100 && b & 0xC0 == 64)
        || (a == 192 && b == 0 && c == 0)
        || (a == 198 && b & 0xFE == 18)
        || a >= 240)
}

/// A refusal by the resolver: a host the plugin may not reach.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct Refused(String);

/// Resolves only listed hosts on port 443 over `https`, and keeps only
/// public addresses.
#[derive(Debug)]
struct ListedResolver {
    hosts: Arc<Vec<String>>,
    /// Every public host, for [`Client::for_plugin_files`].
    any_host: bool,
    inner: DefaultResolver,
}

impl Resolver for ListedResolver {
    fn resolve(
        &self,
        uri: &Uri,
        config: &Config,
        timeout: NextTimeout,
    ) -> std::result::Result<ResolvedSocketAddrs, ureq::Error> {
        let refused = |message: String| ureq::Error::Other(Box::new(Refused(message)));
        let host = uri.host().unwrap_or_default();
        if uri.scheme_str() != Some("https") {
            return Err(refused(format!("{uri} is not an https URL")));
        }
        if uri.port_u16().is_some_and(|port| port != 443) {
            return Err(refused(format!("{uri} is not on port 443")));
        }
        if !self.any_host && !listed(&self.hosts, host) {
            return Err(refused(format!(
                "{host} is not a host the plugin's manifest lists"
            )));
        }
        let resolved = self.inner.resolve(uri, config, timeout)?;
        let mut kept = self.inner.empty();
        for address in resolved.iter().filter(|a| public(a.ip())) {
            kept.push(*address);
        }
        if kept.is_empty() {
            return Err(refused(format!(
                "{host} resolves only to local or private addresses"
            )));
        }
        Ok(kept)
    }
}

/// TLS over the TCP connection, with the operating system's
/// implementation and certificate store, TLS 1.2 or later.
#[derive(Debug, Default)]
struct SystemTls {
    connector: OnceLock<std::result::Result<native_tls::TlsConnector, String>>,
}

fn tls_error(message: impl std::fmt::Display) -> ureq::Error {
    ureq::Error::Io(std::io::Error::other(format!("TLS: {message}")))
}

impl<In: Transport> Connector<In> for SystemTls {
    type Out = TlsTransport;

    fn connect(
        &self,
        details: &ConnectionDetails,
        chained: Option<In>,
    ) -> std::result::Result<Option<TlsTransport>, ureq::Error> {
        let Some(transport) = chained else {
            return Ok(None);
        };
        if !details.needs_tls() {
            return Err(ureq::Error::RequireHttpsOnly(details.uri.to_string()));
        }
        let connector = self
            .connector
            .get_or_init(|| {
                native_tls::TlsConnector::builder()
                    .min_protocol_version(Some(native_tls::Protocol::Tlsv12))
                    .build()
                    .map_err(|e| e.to_string())
            })
            .as_ref()
            .map_err(tls_error)?;
        let host = details.uri.host().unwrap_or_default();
        let mut adapter = TransportAdapter::new(transport.boxed());
        adapter.set_timeout(details.timeout);
        let stream = connector.connect(host, adapter).map_err(|e| match e {
            native_tls::HandshakeError::Failure(e) => tls_error(e),
            native_tls::HandshakeError::WouldBlock(_) => tls_error("the handshake stalled"),
        })?;
        Ok(Some(TlsTransport {
            buffers: LazyBuffers::new(
                details.config.input_buffer_size(),
                details.config.output_buffer_size(),
            ),
            stream,
        }))
    }
}

struct TlsTransport {
    buffers: LazyBuffers,
    stream: native_tls::TlsStream<TransportAdapter>,
}

impl std::fmt::Debug for TlsTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TlsTransport").finish_non_exhaustive()
    }
}

impl Transport for TlsTransport {
    fn buffers(&mut self) -> &mut dyn Buffers {
        &mut self.buffers
    }

    fn transmit_output(
        &mut self,
        amount: usize,
        timeout: NextTimeout,
    ) -> std::result::Result<(), ureq::Error> {
        self.stream.get_mut().set_timeout(timeout);
        self.stream.write_all(&self.buffers.output()[..amount])?;
        Ok(())
    }

    fn await_input(&mut self, timeout: NextTimeout) -> std::result::Result<bool, ureq::Error> {
        self.stream.get_mut().set_timeout(timeout);
        let amount = self.stream.read(self.buffers.input_append_buf())?;
        self.buffers.input_appended(amount);
        Ok(amount > 0)
    }

    fn is_open(&mut self) -> bool {
        self.stream.get_mut().get_mut().is_open()
    }

    fn is_tls(&self) -> bool {
        true
    }
}

pub struct Request {
    pub url: String,
    pub headers: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    pub status: u16,
    /// Names in lower case.
    pub headers: Vec<(String, String)>,
}

/// The requests of one plugin.
pub struct Client {
    agent: ureq::Agent,
    hosts: Arc<Vec<String>>,
    any_host: bool,
}

impl Client {
    /// A client for the plugin `id` at `version` that reaches `hosts`,
    /// patterns that [`check_host_pattern`] accepted.
    #[must_use]
    pub fn new(hosts: Vec<String>, id: &str, version: &str) -> Self {
        Self::build(
            hosts,
            false,
            format!(
                "Angelica/{} (plugin {id}/{version})",
                env!("CARGO_PKG_VERSION")
            ),
        )
    }

    /// A client for the plugin files the installer downloads: every public
    /// host, with the same rules otherwise. A file is trusted for its
    /// author's signature, not for where it came from.
    #[must_use]
    pub fn for_plugin_files() -> Self {
        Self::build(
            Vec::new(),
            true,
            format!("Angelica/{} (installer)", env!("CARGO_PKG_VERSION")),
        )
    }

    fn build(hosts: Vec<String>, any_host: bool, user_agent: String) -> Self {
        let hosts = Arc::new(hosts);
        let config = Config::builder()
            .https_only(true)
            .http_status_as_error(false)
            .proxy(None)
            .max_redirects(MAX_REDIRECTS)
            .max_redirects_will_error(true)
            .user_agent(user_agent)
            .timeout_resolve(Some(CONNECT_TIME))
            .timeout_connect(Some(CONNECT_TIME))
            .timeout_recv_response(Some(RESPONSE_TIME))
            .max_idle_connections_per_host(2)
            .build();
        let agent = ureq::Agent::with_parts(
            config,
            ().chain(TcpConnector::default())
                .chain(SystemTls::default()),
            ListedResolver {
                hosts: hosts.clone(),
                any_host,
                inner: DefaultResolver::default(),
            },
        );
        Self {
            agent,
            hosts,
            any_host,
        }
    }

    /// Checks a request before anything is sent.
    fn check(&self, request: &Request) -> Result<()> {
        if request.url.len() > MAX_URL {
            return Err(Error::invalid(format!("a URL has at most {MAX_URL} bytes")));
        }
        let uri: Uri = request
            .url
            .parse()
            .map_err(|e| Error::invalid(format!("{:?} is not a URL: {e}", request.url)))?;
        if uri.scheme_str() != Some("https") {
            return Err(Error::invalid(format!(
                "{} is not an https URL",
                request.url
            )));
        }
        let Some(authority) = uri.authority() else {
            return Err(Error::invalid(format!("{} has no host", request.url)));
        };
        if authority.as_str().contains('@') {
            return Err(Error::invalid(format!(
                "{} carries a user name or password",
                request.url
            )));
        }
        if authority.port_u16().is_some_and(|port| port != 443) {
            return Err(Error::invalid(format!(
                "{} is not on port 443",
                request.url
            )));
        }
        if !self.any_host && !listed(&self.hosts, authority.host()) {
            return Err(Error::new(
                Kind::PermissionDenied,
                format!(
                    "{} is not a host the plugin's manifest lists",
                    authority.host()
                ),
            ));
        }
        let mut seen = Vec::new();
        for (name, value) in &request.headers {
            if !REQUEST_HEADERS.contains(&name.as_str()) {
                return Err(Error::invalid(format!(
                    "the header {name:?} is not one a plugin sets: {}",
                    REQUEST_HEADERS.join(", ")
                )));
            }
            if seen.contains(name) {
                return Err(Error::invalid(format!("the header {name} is set twice")));
            }
            if value.len() > MAX_HEADER_VALUE || value.chars().any(char::is_control) {
                return Err(Error::invalid(format!(
                    "the header {name} has at most {MAX_HEADER_VALUE} bytes without control characters"
                )));
            }
            seen.push(name.clone());
        }
        Ok(())
    }

    /// Sends the request and returns the final response's head and a reader
    /// of at most `limit` bytes of its body, which stops when `cancelled`
    /// turns true.
    ///
    /// # Errors
    /// An invalid request, an unlisted host or private address, and a
    /// request that did not complete.
    pub fn send(
        &self,
        request: &Request,
        time: Duration,
        limit: u64,
        cancelled: impl Fn() -> bool + Send + 'static,
    ) -> Result<(Head, BodyReader)> {
        self.check(request)?;
        if cancelled() {
            return Err(cancelled_error());
        }
        let mut builder = self.agent.get(&request.url);
        for (name, value) in &request.headers {
            builder = builder.header(name.as_str(), value.as_str());
        }
        let response = builder
            .config()
            .timeout_global(Some(time))
            .build()
            .call()
            .map_err(failure)?;
        let (parts, body) = response.into_parts();
        let head = Head {
            status: parts.status.as_u16(),
            headers: parts
                .headers
                .iter()
                .map(|(name, value)| {
                    (
                        name.as_str().to_owned(),
                        String::from_utf8_lossy(value.as_bytes()).into_owned(),
                    )
                })
                .collect(),
        };
        let reader = BodyReader {
            inner: body.into_with_config().limit(limit).reader(),
            cancelled: Box::new(cancelled),
        };
        Ok((head, reader))
    }

    /// Sends the request and reads the whole body, at most [`MAX_BODY`].
    ///
    /// # Errors
    /// See [`Client::send`]; a larger body is `limit-exceeded`.
    pub fn get(
        &self,
        request: &Request,
        cancelled: impl Fn() -> bool + Send + 'static,
    ) -> Result<(Head, Vec<u8>)> {
        let (head, mut reader) = self.send(request, GET_TIME, MAX_BODY, cancelled)?;
        let mut body = Vec::new();
        reader.read_to_end(&mut body).map_err(read_failure)?;
        Ok((head, body))
    }
}

/// The body of a response.
pub struct BodyReader {
    inner: ureq::BodyReader<'static>,
    cancelled: Box<dyn Fn() -> bool + Send>,
}

impl Read for BodyReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if (self.cancelled)() {
            return Err(std::io::Error::other(Cancelled));
        }
        self.inner.read(buffer)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("the job was cancelled")]
struct Cancelled;

fn cancelled_error() -> Error {
    Error::new(Kind::Network, Cancelled.to_string())
}

/// An error of reading a body.
#[must_use]
pub fn read_failure(error: std::io::Error) -> Error {
    if error
        .get_ref()
        .is_some_and(|inner| inner.downcast_ref::<Cancelled>().is_some())
    {
        return cancelled_error();
    }
    failure(ureq::Error::from(error))
}

fn failure(error: ureq::Error) -> Error {
    match error {
        ureq::Error::Other(inner) if inner.downcast_ref::<Refused>().is_some() => {
            Error::new(Kind::PermissionDenied, inner.to_string())
        }
        ureq::Error::BodyExceedsLimit(limit) => Error::new(
            Kind::LimitExceeded,
            format!("the body is larger than {limit} bytes"),
        ),
        ureq::Error::RequireHttpsOnly(url) => Error::new(
            Kind::PermissionDenied,
            format!("redirected to {url}, which is not https"),
        ),
        ureq::Error::TooManyRedirects => Error::new(
            Kind::Network,
            format!("more than {MAX_REDIRECTS} redirects"),
        ),
        ureq::Error::Timeout(_) => Error::new(Kind::Network, "the request took too long"),
        other => Error::new(Kind::Network, other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> Client {
        Client::new(
            vec!["example.com".into(), "*.cdn.example.org".into()],
            "p",
            "1.0.0",
        )
    }

    fn request(url: &str) -> Request {
        Request {
            url: url.into(),
            headers: Vec::new(),
        }
    }

    #[test]
    fn host_patterns_are_names() {
        for pattern in [
            "example.com",
            "*.example.com",
            "a-b.c1.example",
            "xn--80ak6aa92e.com",
        ] {
            assert_eq!(check_host_pattern(pattern), Ok(()), "{pattern}");
        }
        for pattern in [
            "",
            "com",
            "*.com",
            "localhost",
            "Example.com",
            "-a.com",
            "a..com",
            "1.2.3.4",
            "a.com.",
            "*.*.com",
            "a_b.com",
            "пример.рф",
            "[::1]",
            "a.com:443",
        ] {
            assert!(check_host_pattern(pattern).is_err(), "{pattern}");
        }
    }

    #[test]
    fn wildcards_match_subdomains_only() {
        let hosts = ["example.com".to_owned(), "*.cdn.example.org".to_owned()];
        assert!(listed(&hosts, "example.com"));
        assert!(listed(&hosts, "EXAMPLE.com"));
        assert!(!listed(&hosts, "www.example.com"));
        assert!(listed(&hosts, "a.cdn.example.org"));
        assert!(listed(&hosts, "a.b.cdn.example.org"));
        assert!(!listed(&hosts, "cdn.example.org"));
        assert!(!listed(&hosts, "evilcdn.example.org"));
        assert!(!listed(&hosts, "example.com.evil.net"));
    }

    #[test]
    fn only_public_addresses_are_reached() {
        for address in [
            "93.184.215.14",
            "1.1.1.1",
            "2606:4700::1111",
            "::ffff:8.8.8.8",
        ] {
            assert!(public(address.parse().unwrap()), "{address}");
        }
        for address in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "255.255.255.255",
            "224.0.0.1",
            "192.0.0.1",
            "198.18.0.1",
            "240.0.0.1",
            "192.0.2.1",
            "::1",
            "::",
            "fd00::1",
            "fe80::1",
            "ff02::1",
            "::ffff:127.0.0.1",
            "::127.0.0.1",
            "64:ff9b::7f00:1",
            "2002:7f00:1::",
            "2001:db8::1",
        ] {
            assert!(!public(address.parse().unwrap()), "{address}");
        }
    }

    #[test]
    fn requests_are_checked_before_anything_is_sent() {
        let client = client();
        let refused = |request: Request, kind: Kind, problem: &str| {
            let error = client.check(&request).unwrap_err();
            assert_eq!(error.kind, kind, "{}", error.message);
            assert!(error.message.contains(problem), "{}", error.message);
        };
        assert!(
            client
                .check(&request("https://example.com/feed.json"))
                .is_ok()
        );
        assert!(
            client
                .check(&request("https://a.cdn.example.org:443/x"))
                .is_ok()
        );
        refused(
            request("http://example.com/"),
            Kind::InvalidArgument,
            "not an https URL",
        );
        refused(
            request("https://example.com:8443/"),
            Kind::InvalidArgument,
            "port 443",
        );
        refused(
            request("https://user:pw@example.com/"),
            Kind::InvalidArgument,
            "user name",
        );
        refused(
            request("example.com/x"),
            Kind::InvalidArgument,
            "is not a URL",
        );
        refused(
            request("https://example.net/"),
            Kind::PermissionDenied,
            "example.net is not",
        );
        refused(
            request("https://127.0.0.1/"),
            Kind::PermissionDenied,
            "127.0.0.1 is not",
        );
        let with = |name: &str, value: &str| Request {
            url: "https://example.com/".into(),
            headers: vec![(name.into(), value.into())],
        };
        assert!(client.check(&with("if-none-match", "\"abc\"")).is_ok());
        refused(
            with("cookie", "a=b"),
            Kind::InvalidArgument,
            "not one a plugin sets",
        );
        refused(
            with("Accept", "*/*"),
            Kind::InvalidArgument,
            "not one a plugin sets",
        );
        refused(
            with("range", "bytes=0-1\r\nx: y"),
            Kind::InvalidArgument,
            "control",
        );
        let mut twice = with("accept", "a");
        twice.headers.push(("accept".into(), "b".into()));
        refused(twice, Kind::InvalidArgument, "twice");
    }

    #[test]
    fn redirects_and_resolution_follow_the_same_rules() {
        let resolver = ListedResolver {
            hosts: Arc::new(vec!["example.com".into()]),
            any_host: false,
            inner: DefaultResolver::default(),
        };
        let config = Config::default();
        let resolve = |url: &str| {
            let uri: Uri = url.parse().unwrap();
            resolver
                .resolve(
                    &uri,
                    &config,
                    NextTimeout {
                        after: ureq::unversioned::transport::time::Duration::NotHappening,
                        reason: ureq::Timeout::Global,
                    },
                )
                .map_err(failure)
        };
        for (url, problem) in [
            ("https://example.net/", "example.net is not a host"),
            ("http://example.com/", "is not an https URL"),
            ("https://example.com:8080/", "is not on port 443"),
        ] {
            let error = resolve(url).unwrap_err();
            assert_eq!(error.kind, Kind::PermissionDenied);
            assert!(error.message.contains(problem), "{}", error.message);
        }
    }

    #[test]
    fn plugin_files_come_from_any_public_host_only() {
        let client = Client::for_plugin_files();
        for (url, kind) in [
            ("https://127.0.0.1/timers.angelica", Kind::PermissionDenied),
            ("https://localhost/timers.angelica", Kind::PermissionDenied),
            ("http://example.org/timers.angelica", Kind::InvalidArgument),
            (
                "https://example.org:8443/timers.angelica",
                Kind::InvalidArgument,
            ),
        ] {
            let error = client
                .send(&request(url), GET_TIME, MAX_BODY, || false)
                .err()
                .unwrap();
            assert_eq!(error.kind, kind, "{url}: {}", error.message);
        }
        assert!(client.check(&request("https://any.example.org/")).is_ok());
    }

    #[test]
    fn a_cancelled_job_sends_nothing() {
        let error = client()
            .get(&request("https://example.com/"), || true)
            .unwrap_err();
        assert_eq!(error.kind, Kind::Network);
        assert_eq!(error.message, "the job was cancelled");
    }

    /// Needs the internet.
    #[test]
    #[ignore = "needs the internet"]
    fn gets_a_page_over_the_internet() {
        let client = client();
        let (head, body) = client
            .get(&request("https://example.com/"), || false)
            .unwrap();
        assert_eq!(head.status, 200);
        assert!(String::from_utf8_lossy(&body).contains("Example Domain"));
        assert!(head.headers.iter().any(|(name, _)| name == "content-type"));
    }

    /// Needs the internet.
    #[test]
    #[ignore = "needs the internet"]
    fn redirects_and_certificates_are_checked_over_the_internet() {
        let client = Client::new(
            vec!["github.com".into(), "expired.badssl.com".into()],
            "p",
            "1.0.0",
        );
        let error = client
            .get(
                &request("https://github.com/rust-lang/rust/archive/refs/tags/1.0.0.tar.gz"),
                || false,
            )
            .unwrap_err();
        assert_eq!(error.kind, Kind::PermissionDenied, "{}", error.message);
        assert!(
            error.message.contains("codeload.github.com"),
            "{}",
            error.message
        );
        let error = client
            .get(&request("https://expired.badssl.com/"), || false)
            .unwrap_err();
        assert_eq!(error.kind, Kind::Network, "{}", error.message);
        assert!(error.message.contains("TLS"), "{}", error.message);
    }
}
