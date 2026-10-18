use std::net::IpAddr;
use std::thread;

use log::{debug, error, info, warn};

use crate::cli::DaemonOptions;
use crate::error::Error;
use crate::ip::IpSource;
use crate::provider::Provider;

/// Result of a single daemon poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollOutcome {
    /// The discovered address matches the remembered one; no update was made.
    Unchanged {
        /// Address that is already up to date.
        ip: IpAddr,
    },
    /// The provider accepted a new address, which should now be remembered.
    Updated {
        /// Address that was written to the provider.
        ip: IpAddr,
    },
}

/// Discovers the public IP once and updates the provider.
///
/// This is the stateless mode: nothing is remembered between calls.
pub fn run_once<S, P>(source: &S, provider: &P) -> Result<IpAddr, Error<P::Error>>
where
    S: IpSource,
    P: Provider,
{
    let ip = source.public_ip()?;
    provider.update(ip).map_err(Error::Provider)?;
    info!("updated {} record to {ip}", record_kind(ip));
    Ok(ip)
}

/// Performs one poll, updating the provider only when `last` differs from the
/// discovered address.
///
/// The caller decides what to remember; this function performs no I/O besides
/// IP discovery and the provider update.
pub fn poll_once<S, P>(
    source: &S,
    provider: &P,
    last: Option<IpAddr>,
) -> Result<PollOutcome, Error<P::Error>>
where
    S: IpSource,
    P: Provider,
{
    let ip = source.public_ip()?;

    if last == Some(ip) {
        return Ok(PollOutcome::Unchanged { ip });
    }

    provider.update(ip).map_err(Error::Provider)?;
    Ok(PollOutcome::Updated { ip })
}

/// Runs polls forever, remembering the last successfully updated address in
/// memory only.
///
/// Errors never terminate the loop; the next poll retries.
pub fn run_daemon<S, P>(source: &S, provider: &P, options: DaemonOptions) -> !
where
    S: IpSource,
    P: Provider,
{
    let mut last: Option<IpAddr> = None;

    loop {
        match daemon_step(source, provider, &mut last) {
            Ok(PollOutcome::Unchanged { ip }) => {
                debug!("public IP is still {ip}; skipping update");
            }
            Ok(PollOutcome::Updated { ip }) => {
                info!("updated {} record to {ip}", record_kind(ip));
            }
            Err(err) => error!("{err}; retrying on the next poll"),
        }

        thread::sleep(options.interval());
    }
}

/// One iteration of the daemon loop, without sleeping.
///
/// `last` is only advanced after a successful update.
fn daemon_step<S, P>(
    source: &S,
    provider: &P,
    last: &mut Option<IpAddr>,
) -> Result<PollOutcome, Error<P::Error>>
where
    S: IpSource,
    P: Provider,
{
    let previous = *last;
    let outcome = poll_once(source, provider, previous)?;

    if let PollOutcome::Updated { ip } = outcome {
        if let Some(previous) = previous.filter(|prev| prev.is_ipv4() != ip.is_ipv4()) {
            warn!(
                "address family changed from {previous} to {ip}; {}-family records are no longer maintained by this process",
                if previous.is_ipv4() { "IPv4" } else { "IPv6" }
            );
        }
        *last = Some(ip);
    }

    Ok(outcome)
}

fn record_kind(ip: IpAddr) -> &'static str {
    match ip {
        IpAddr::V4(_) => "A",
        IpAddr::V6(_) => "AAAA",
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::net::Ipv4Addr;

    use super::*;

    use crate::error::IpError;

    #[derive(Debug, thiserror::Error)]
    #[error("provider refused the update")]
    struct UpdateFailed;

    struct FakeSource {
        results: RefCell<Vec<Result<IpAddr, IpError>>>,
        calls: Cell<usize>,
    }

    impl FakeSource {
        fn returning(results: Vec<Result<IpAddr, IpError>>) -> Self {
            Self {
                results: RefCell::new(results),
                calls: Cell::new(0),
            }
        }

        fn once(ip: IpAddr) -> Self {
            Self::returning(vec![Ok(ip)])
        }

        fn calls(&self) -> usize {
            self.calls.get()
        }
    }

    impl IpSource for FakeSource {
        fn public_ip(&self) -> Result<IpAddr, IpError> {
            self.calls.set(self.calls.get() + 1);
            self.results
                .borrow_mut()
                .pop()
                .expect("FakeSource ran out of results")
        }
    }

    #[derive(Default)]
    struct FakeProvider {
        updates: RefCell<Vec<IpAddr>>,
        fail: bool,
    }

    impl FakeProvider {
        fn failing() -> Self {
            Self {
                updates: RefCell::default(),
                fail: true,
            }
        }

        fn updates(&self) -> Vec<IpAddr> {
            self.updates.borrow().clone()
        }
    }

    impl Provider for FakeProvider {
        type Error = UpdateFailed;

        fn update(&self, ip: IpAddr) -> Result<(), Self::Error> {
            if self.fail {
                return Err(UpdateFailed);
            }
            self.updates.borrow_mut().push(ip);
            Ok(())
        }
    }

    fn v4(last: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(8, 8, 8, last))
    }

    fn discovery_failed() -> IpError {
        IpError::NotPublic { ip: v4(1) }
    }

    #[test]
    fn run_once_updates_the_discovered_address() {
        let source = FakeSource::once(v4(8));
        let provider = FakeProvider::default();

        assert_eq!(run_once(&source, &provider).unwrap(), v4(8));
        assert_eq!(source.calls(), 1);
        assert_eq!(provider.updates(), vec![v4(8)]);
    }

    #[test]
    fn run_once_propagates_discovery_failures_without_updating() {
        let source = FakeSource::returning(vec![Err(discovery_failed())]);
        let provider = FakeProvider::default();

        assert!(matches!(run_once(&source, &provider), Err(Error::Ip(_))));
        assert!(provider.updates().is_empty());
    }

    #[test]
    fn run_once_propagates_provider_failures() {
        let source = FakeSource::once(v4(8));
        let provider = FakeProvider::failing();

        assert!(matches!(
            run_once(&source, &provider),
            Err(Error::Provider(_))
        ));
    }

    #[test]
    fn poll_once_updates_when_nothing_was_remembered() {
        let source = FakeSource::once(v4(8));
        let provider = FakeProvider::default();

        assert_eq!(
            poll_once(&source, &provider, None).unwrap(),
            PollOutcome::Updated { ip: v4(8) }
        );
        assert_eq!(provider.updates(), vec![v4(8)]);
    }

    #[test]
    fn poll_once_skips_an_unchanged_address() {
        let source = FakeSource::once(v4(8));
        let provider = FakeProvider::default();

        assert_eq!(
            poll_once(&source, &provider, Some(v4(8))).unwrap(),
            PollOutcome::Unchanged { ip: v4(8) }
        );
        assert!(provider.updates().is_empty());
    }

    #[test]
    fn poll_once_updates_a_changed_address() {
        let source = FakeSource::once(v4(9));
        let provider = FakeProvider::default();

        assert_eq!(
            poll_once(&source, &provider, Some(v4(8))).unwrap(),
            PollOutcome::Updated { ip: v4(9) }
        );
        assert_eq!(provider.updates(), vec![v4(9)]);
    }

    #[test]
    fn poll_once_does_not_update_when_discovery_fails() {
        let source = FakeSource::returning(vec![Err(discovery_failed())]);
        let provider = FakeProvider::default();

        assert!(matches!(
            poll_once(&source, &provider, Some(v4(8))),
            Err(Error::Ip(_))
        ));
        assert!(provider.updates().is_empty());
    }

    #[test]
    fn poll_once_reports_provider_failures() {
        let source = FakeSource::once(v4(9));
        let provider = FakeProvider::failing();

        assert!(matches!(
            poll_once(&source, &provider, Some(v4(8))),
            Err(Error::Provider(_))
        ));
    }

    #[test]
    fn daemon_step_remembers_only_successful_updates() {
        let source = FakeSource::returning(vec![Ok(v4(9))]);
        let provider = FakeProvider::failing();
        let mut last = Some(v4(8));

        assert!(daemon_step(&source, &provider, &mut last).is_err());
        assert_eq!(last, Some(v4(8)));
    }

    #[test]
    fn daemon_step_remembers_a_new_address() {
        let source = FakeSource::returning(vec![Ok(v4(9))]);
        let provider = FakeProvider::default();
        let mut last = Some(v4(8));

        assert_eq!(
            daemon_step(&source, &provider, &mut last).unwrap(),
            PollOutcome::Updated { ip: v4(9) }
        );
        assert_eq!(last, Some(v4(9)));
    }

    #[test]
    fn daemon_step_keeps_the_address_when_discovery_fails() {
        let source = FakeSource::returning(vec![Err(discovery_failed())]);
        let provider = FakeProvider::default();
        let mut last = Some(v4(8));

        assert!(daemon_step(&source, &provider, &mut last).is_err());
        assert_eq!(last, Some(v4(8)));
        assert!(provider.updates().is_empty());
    }
}
