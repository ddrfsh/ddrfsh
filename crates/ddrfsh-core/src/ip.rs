use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use clap::ValueEnum;

use crate::error::IpError;
use crate::http;

/// Default HTTPS echo endpoint used to discover the public IPv4 address.
pub const DEFAULT_IP_ENDPOINT_V4: &str = "https://api.ipify.org";

/// Default HTTPS echo endpoint used to discover the public IPv6 address.
pub const DEFAULT_IP_ENDPOINT_V6: &str = "https://api6.ipify.org";

/// Timeout applied to the whole public IP request.
pub const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// Upper bound on the response body accepted from an echo endpoint.
///
/// The longest textual IP address is 45 characters, so this only guards
/// against an endpoint returning something unexpected.
const MAX_BODY_BYTES: u64 = 64;

/// Address family to discover and update.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum IpFamily {
    /// Discover an IPv4 address and update the A record.
    #[default]
    Ipv4,
    /// Discover an IPv6 address and update the AAAA record.
    Ipv6,
}

impl IpFamily {
    /// Human-readable label used in logs and error messages.
    pub fn label(self) -> &'static str {
        match self {
            Self::Ipv4 => "IPv4",
            Self::Ipv6 => "IPv6",
        }
    }

    /// DNS record type that corresponds to this family.
    pub fn record_kind(self) -> &'static str {
        match self {
            Self::Ipv4 => "A",
            Self::Ipv6 => "AAAA",
        }
    }

    fn accepts(self, ip: IpAddr) -> bool {
        match self {
            Self::Ipv4 => ip.is_ipv4(),
            Self::Ipv6 => ip.is_ipv6(),
        }
    }
}

impl fmt::Display for IpFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Returns the default echo endpoint for `family`.
pub fn default_endpoint(family: IpFamily) -> &'static str {
    match family {
        IpFamily::Ipv4 => DEFAULT_IP_ENDPOINT_V4,
        IpFamily::Ipv6 => DEFAULT_IP_ENDPOINT_V6,
    }
}

/// Source of the machine's public IP address.
pub trait IpSource {
    /// Returns the public IP address as observed from the outside.
    fn public_ip(&self) -> Result<IpAddr, IpError>;
}

/// [`IpSource`] backed by an HTTPS echo endpoint.
pub struct HttpIpSource {
    endpoint: String,
    family: IpFamily,
    agent: ureq::Agent,
}

impl HttpIpSource {
    /// Uses the default endpoint for `family` and [`DEFAULT_HTTP_TIMEOUT`].
    pub fn for_family(family: IpFamily) -> Self {
        Self::with_endpoint(default_endpoint(family), family, DEFAULT_HTTP_TIMEOUT)
    }

    /// Uses a caller-provided endpoint, for self-hosted echo services and tests.
    pub fn with_endpoint(endpoint: impl Into<String>, family: IpFamily, timeout: Duration) -> Self {
        Self {
            endpoint: endpoint.into(),
            family,
            agent: http::http_agent(timeout),
        }
    }
}

impl IpSource for HttpIpSource {
    fn public_ip(&self) -> Result<IpAddr, IpError> {
        let mut response =
            self.agent
                .get(&self.endpoint)
                .call()
                .map_err(|err| IpError::Request {
                    endpoint: self.endpoint.clone(),
                    family: self.family,
                    message: transport_message(self.family, &self.endpoint, &err),
                })?;

        let status = response.status().as_u16();
        if !(200..=299).contains(&status) {
            return Err(IpError::Status {
                endpoint: self.endpoint.clone(),
                status,
            });
        }

        let body = response
            .body_mut()
            .with_config()
            .limit(MAX_BODY_BYTES)
            .read_to_string()
            .map_err(|err| IpError::Malformed {
                endpoint: self.endpoint.clone(),
                message: err.to_string(),
            })?;

        parse_public_ip(&body, self.family, &self.endpoint)
    }
}

/// Parses an echo endpoint response into a public IP address.
///
/// `endpoint` is only used to build error messages.
pub fn parse_public_ip(body: &str, family: IpFamily, endpoint: &str) -> Result<IpAddr, IpError> {
    let raw = body.trim();
    let ip: IpAddr = raw.parse().map_err(|_| IpError::Malformed {
        endpoint: endpoint.to_owned(),
        message: if raw.is_empty() {
            "response body is empty".to_owned()
        } else {
            "response body is not an IP address".to_owned()
        },
    })?;

    // An IPv4-mapped IPv6 address is an IPv4 address in disguise; treating it
    // as IPv6 would write a bogus AAAA record.
    let ip = match ip {
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => ip,
        },
        IpAddr::V4(_) => ip,
    };

    if !family.accepts(ip) {
        return Err(IpError::UnexpectedFamily {
            endpoint: endpoint.to_owned(),
            family,
            got: ip,
        });
    }

    if !is_public(ip) {
        return Err(IpError::NotPublic { ip });
    }

    Ok(ip)
}

fn transport_message(family: IpFamily, endpoint: &str, err: &ureq::Error) -> String {
    let mut message = err.to_string();

    // A failure against the built-in endpoint usually means the host lacks
    // connectivity for that family, so point at the other one.
    if endpoint == default_endpoint(family) {
        let alternative = match family {
            IpFamily::Ipv4 => "ipv6",
            IpFamily::Ipv6 => "ipv4",
        };
        message.push_str(&format!(
            " (this host may not have {} connectivity; try --ip-family {alternative})",
            family.label()
        ));
    }

    message
}

fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => is_public_v6(v6),
    }
}

/// Rejects address ranges that cannot be a machine's public address.
///
/// `Ipv4Addr::is_global` and `Ipv6Addr::is_global` are still unstable, so this
/// combines the stable classifier methods instead.
fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [first, second, ..] = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_multicast()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_documentation()
        // 0.0.0.0/8, "this network"
        || first == 0
        // 100.64.0.0/10, carrier-grade NAT
        || (first == 100 && (64..=127).contains(&second))
        // 240.0.0.0/4, reserved
        || first >= 240)
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    let [first, second, ..] = ip.segments();
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || ip.is_unique_local()
        || ip.is_unicast_link_local()
        // 2001:db8::/32, documentation
        || (first == 0x2001 && second == 0x0db8))
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::thread::{self, JoinHandle};

    use super::*;

    const ENDPOINT: &str = "https://example.invalid/";

    fn serve(status: u16, body: &str) -> (String, JoinHandle<()>) {
        serve_after(status, body, Duration::ZERO)
    }

    fn serve_after(status: u16, body: &str, delay: Duration) -> (String, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let body = body.to_owned();
        let handle = thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                loop {
                    line.clear();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                }
                if !delay.is_zero() {
                    thread::sleep(delay);
                }
                let response = format!(
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        (format!("http://{addr}/"), handle)
    }

    #[test]
    fn parses_ipv4_with_surrounding_whitespace() {
        let ip = parse_public_ip("  8.8.8.8\r\n", IpFamily::Ipv4, ENDPOINT).unwrap();
        assert_eq!(ip, "8.8.8.8".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn parses_ipv6() {
        let ip = parse_public_ip("2001:4860:4860::8888", IpFamily::Ipv6, ENDPOINT).unwrap();
        assert_eq!(ip, "2001:4860:4860::8888".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn rejects_empty_body() {
        let err = parse_public_ip("\n", IpFamily::Ipv4, ENDPOINT).unwrap_err();
        assert!(matches!(err, IpError::Malformed { .. }), "{err}");
    }

    #[test]
    fn rejects_non_ip_body() {
        let err = parse_public_ip("<html>nope</html>", IpFamily::Ipv4, ENDPOINT).unwrap_err();
        assert!(matches!(err, IpError::Malformed { .. }), "{err}");
    }

    #[test]
    fn rejects_address_of_the_other_family() {
        let err = parse_public_ip("8.8.8.8", IpFamily::Ipv6, ENDPOINT).unwrap_err();
        assert!(matches!(err, IpError::UnexpectedFamily { .. }), "{err}");
    }

    #[test]
    fn rejects_ipv4_mapped_address() {
        let err = parse_public_ip("::ffff:8.8.8.8", IpFamily::Ipv6, ENDPOINT).unwrap_err();
        assert!(matches!(err, IpError::UnexpectedFamily { .. }), "{err}");
    }

    #[test]
    fn rejects_non_public_addresses() {
        for raw in [
            "10.0.0.1",
            "172.16.0.1",
            "192.168.1.1",
            "127.0.0.1",
            "169.254.1.1",
            "224.0.0.1",
            "0.0.0.0",
            "255.255.255.255",
            "192.0.2.1",
            "100.64.0.1",
            "240.0.0.1",
        ] {
            let err = parse_public_ip(raw, IpFamily::Ipv4, ENDPOINT).unwrap_err();
            assert!(matches!(err, IpError::NotPublic { .. }), "{raw}: {err}");
        }

        for raw in ["::1", "::", "ff02::1", "fd00::1", "fe80::1", "2001:db8::1"] {
            let err = parse_public_ip(raw, IpFamily::Ipv6, ENDPOINT).unwrap_err();
            assert!(matches!(err, IpError::NotPublic { .. }), "{raw}: {err}");
        }
    }

    #[test]
    fn default_endpoints_match_the_family() {
        assert_eq!(default_endpoint(IpFamily::Ipv4), DEFAULT_IP_ENDPOINT_V4);
        assert_eq!(default_endpoint(IpFamily::Ipv6), DEFAULT_IP_ENDPOINT_V6);
    }

    #[test]
    fn suggests_the_other_family_for_default_endpoints() {
        let err = ureq::Error::BadUri("bad".to_owned());

        let message = transport_message(IpFamily::Ipv6, DEFAULT_IP_ENDPOINT_V6, &err);
        assert!(message.contains("try --ip-family ipv4"), "{message}");

        let custom = transport_message(IpFamily::Ipv6, "http://127.0.0.1:1/", &err);
        assert!(!custom.contains("--ip-family"), "{custom}");
    }

    #[test]
    fn http_source_returns_the_discovered_address() {
        let (endpoint, server) = serve(200, "8.8.8.8\n");
        let source = HttpIpSource::with_endpoint(endpoint, IpFamily::Ipv4, DEFAULT_HTTP_TIMEOUT);
        assert_eq!(source.public_ip().unwrap().to_string(), "8.8.8.8");
        server.join().unwrap();
    }

    #[test]
    fn http_source_reports_non_success_status() {
        let (endpoint, server) = serve(503, "");
        let source = HttpIpSource::with_endpoint(endpoint, IpFamily::Ipv4, DEFAULT_HTTP_TIMEOUT);
        let err = source.public_ip().unwrap_err();
        assert!(matches!(err, IpError::Status { status: 503, .. }), "{err}");
        server.join().unwrap();
    }

    #[test]
    fn http_source_does_not_follow_redirects() {
        let (endpoint, server) = serve(302, "");
        let source = HttpIpSource::with_endpoint(endpoint, IpFamily::Ipv4, DEFAULT_HTTP_TIMEOUT);
        let err = source.public_ip().unwrap_err();
        assert!(matches!(err, IpError::Status { status: 302, .. }), "{err}");
        server.join().unwrap();
    }

    #[test]
    fn http_source_rejects_oversized_bodies() {
        let (endpoint, server) = serve(200, &"1".repeat(100));
        let source = HttpIpSource::with_endpoint(endpoint, IpFamily::Ipv4, DEFAULT_HTTP_TIMEOUT);
        let err = source.public_ip().unwrap_err();
        assert!(matches!(err, IpError::Malformed { .. }), "{err}");
        server.join().unwrap();
    }

    #[test]
    fn http_source_reports_unreachable_endpoints() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        drop(listener);

        let source = HttpIpSource::with_endpoint(endpoint, IpFamily::Ipv6, DEFAULT_HTTP_TIMEOUT);
        let err = source.public_ip().unwrap_err();
        match err {
            IpError::Request {
                family, message, ..
            } => {
                assert_eq!(family, IpFamily::Ipv6);
                assert!(!message.is_empty());
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn http_source_times_out() {
        let (endpoint, server) = serve_after(200, "8.8.8.8", Duration::from_millis(400));
        let source =
            HttpIpSource::with_endpoint(endpoint, IpFamily::Ipv4, Duration::from_millis(50));
        let err = source.public_ip().unwrap_err();
        assert!(matches!(err, IpError::Request { .. }), "{err}");
        server.join().unwrap();
    }
}
