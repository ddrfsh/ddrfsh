//! Shared logic for `ddrfsh` DDNS clients.
//!
//! This crate holds the code that is common to every provider binary:
//! public IP discovery, the provider abstraction, execution modes, and
//! change detection. It must never contain provider-specific authentication,
//! signing, or response mapping.
//!
//! # Execution modes
//!
//! - [`run_once`] discovers the public IP and calls the provider exactly once.
//!   It is stateless and never persists anything.
//! - [`run_daemon`] polls forever, remembering the last successfully updated
//!   address **in memory only**. A restart performs one redundant, idempotent
//!   update.
//!
//! # Example
//!
//! ```no_run
//! use ddrfsh_core::{run_once, HttpIpSource, IpFamily, Provider};
//! use std::net::IpAddr;
//!
//! struct MyProvider;
//!
//! impl Provider for MyProvider {
//!     type Error = std::io::Error;
//!
//!     fn update(&self, _ip: IpAddr) -> Result<(), Self::Error> {
//!         Ok(())
//!     }
//! }
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let source = HttpIpSource::for_family(IpFamily::Ipv4);
//!     run_once(&source, &MyProvider)?;
//!     Ok(())
//! }
//! ```

mod cli;
mod error;
mod http;
mod ip;
mod logging;
mod provider;
mod run;

pub use cli::{CommonArgs, DEFAULT_INTERVAL_SECS, DaemonOptions, MIN_INTERVAL_SECS};
pub use error::{ConfigError, Error, IpError};
pub use ip::{
    DEFAULT_HTTP_TIMEOUT, DEFAULT_IP_ENDPOINT_V4, DEFAULT_IP_ENDPOINT_V6, HttpIpSource, IpFamily,
    IpSource, default_endpoint, parse_public_ip,
};
pub use logging::init_logging;
pub use provider::Provider;
pub use run::{PollOutcome, poll_once, run_daemon, run_once};
