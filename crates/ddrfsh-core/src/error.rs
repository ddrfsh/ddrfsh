use std::net::IpAddr;

use crate::ip::IpFamily;

/// Everything that can go wrong while discovering the public IP address.
#[derive(Debug, thiserror::Error)]
pub enum IpError {
    /// The request to the echo endpoint could not be completed.
    #[error("failed to discover an {family} address via {endpoint}: {message}")]
    Request {
        /// Endpoint that was queried.
        endpoint: String,
        /// Address family that was requested.
        family: IpFamily,
        /// Transport-level reason reported by the HTTP client.
        message: String,
    },

    /// The echo endpoint answered with a status outside `200..=299`.
    #[error("{endpoint} returned HTTP status {status}")]
    Status {
        /// Endpoint that was queried.
        endpoint: String,
        /// Status code returned by the endpoint.
        status: u16,
    },

    /// The response body could not be turned into a usable IP address.
    #[error("{endpoint} did not return a valid IP address: {message}")]
    Malformed {
        /// Endpoint that was queried.
        endpoint: String,
        /// Why the body was rejected.
        message: String,
    },

    /// The endpoint returned an address of the other family, which usually
    /// means the endpoint or an intermediate proxy is misconfigured.
    #[error("{endpoint} returned {got}, but --ip-family requires {family}")]
    UnexpectedFamily {
        /// Endpoint that was queried.
        endpoint: String,
        /// Address family that was requested.
        family: IpFamily,
        /// Address that the endpoint actually returned.
        got: IpAddr,
    },

    /// The address parsed correctly but is not globally routable.
    #[error("{ip} is not a publicly routable address")]
    NotPublic {
        /// Address that was rejected.
        ip: IpAddr,
    },
}

/// Errors from combining IP discovery with a provider update.
#[derive(Debug, thiserror::Error)]
pub enum Error<E>
where
    E: std::error::Error + 'static,
{
    /// Public IP discovery failed.
    #[error(transparent)]
    Ip(#[from] IpError),

    /// The provider rejected or failed the update.
    #[error("provider update failed: {0}")]
    Provider(#[source] E),
}

/// Errors caused by invalid command-line configuration.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// `--interval` was provided without `--daemon`.
    #[error("--interval requires --daemon")]
    IntervalWithoutDaemon,

    /// The polling interval must be at least one second.
    #[error("invalid --interval {0}: the polling interval must be at least 1 second")]
    IntervalTooSmall(u64),
}
