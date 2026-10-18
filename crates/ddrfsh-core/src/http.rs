use std::time::Duration;

use ureq::Agent;

/// Builds the HTTP agent used for public IP discovery.
///
/// Redirects are disabled so a misconfigured endpoint cannot silently send the
/// request to a different host, and non-2xx statuses are returned to the caller
/// instead of being turned into transport errors.
pub(crate) fn agent(timeout: Duration) -> Agent {
    Agent::config_builder()
        .timeout_global(Some(timeout))
        .max_redirects(0)
        .http_status_as_error(false)
        .user_agent(concat!("ddrfsh/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}
