use std::net::IpAddr;

/// DNS record type managed for a given address family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RecordType {
    A,
    Aaaa,
}

impl RecordType {
    /// Returns the record type that matches `ip`.
    pub(crate) fn for_ip(ip: IpAddr) -> Self {
        match ip {
            IpAddr::V4(_) => Self::A,
            IpAddr::V6(_) => Self::Aaaa,
        }
    }

    /// Returns the name Cloudflare uses for this record type.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::Aaaa => "AAAA",
        }
    }
}

/// A DNS record returned by the Cloudflare API.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Record {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) kind: String,
    pub(crate) content: String,
}

/// The subset of the Cloudflare DNS API this client needs.
///
/// Implemented by [`crate::http_api::HttpApi`] for real requests and by a fake
/// in tests, so provider decisions can be verified without network access.
pub(crate) trait DnsApi {
    /// Lists records matching `name` and `kind`.
    fn find_records(
        &self,
        zone: &str,
        name: &str,
        kind: RecordType,
    ) -> Result<Vec<Record>, ApiError>;

    /// Updates the record identified by `id` to point at `ip`.
    fn update_record(&self, zone: &str, id: &str, ip: IpAddr) -> Result<(), ApiError>;

    /// Creates a record for `name` pointing at `ip`.
    fn create_record(
        &self,
        zone: &str,
        name: &str,
        kind: RecordType,
        ip: IpAddr,
    ) -> Result<(), ApiError>;
}

/// Errors reported by the Cloudflare API layer.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ApiError {
    /// The request could not be completed.
    #[error("request to {url} failed: {message}")]
    Request {
        /// Request URL, without credentials.
        url: String,
        /// Transport reason reported by the HTTP client.
        message: String,
    },

    /// Cloudflare answered with an error.
    #[error("{url} returned HTTP {status}: {details}")]
    Status {
        /// Request URL, without credentials.
        url: String,
        /// HTTP status code.
        status: u16,
        /// Messages reported by Cloudflare, or a short body summary.
        details: String,
    },

    /// Cloudflare answered with something this client cannot interpret.
    #[error("{url} returned an unexpected response: {message}")]
    Malformed {
        /// Request URL, without credentials.
        url: String,
        /// Why the response was rejected.
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_address_families_to_record_types() {
        let v4: IpAddr = "8.8.8.8".parse().unwrap();
        let v6: IpAddr = "2001:4860:4860::8888".parse().unwrap();

        assert_eq!(RecordType::for_ip(v4), RecordType::A);
        assert_eq!(RecordType::for_ip(v6), RecordType::Aaaa);
        assert_eq!(RecordType::A.as_str(), "A");
        assert_eq!(RecordType::Aaaa.as_str(), "AAAA");
    }
}
