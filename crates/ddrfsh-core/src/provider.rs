use std::net::IpAddr;

/// Performs a single provider update for `ip`.
///
/// Implementations map [`IpAddr::V4`] to an A record and [`IpAddr::V6`] to an
/// AAAA record.
pub trait Provider {
    /// Provider-specific error type.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Updates the provider's DNS record to point at `ip`.
    fn update(&self, ip: IpAddr) -> Result<(), Self::Error>;
}
