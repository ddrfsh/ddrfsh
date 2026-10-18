use std::net::IpAddr;

use log::debug;

use crate::api::{ApiError, DnsApi, RecordType};

/// [`ddrfsh_core::Provider`] implementation for Cloudflare DNS records.
pub(crate) struct CloudflareProvider<A> {
    api: A,
    zone: String,
    record: String,
    create: bool,
}

impl<A: DnsApi> CloudflareProvider<A> {
    /// Builds a provider that updates `record` in `zone`.
    ///
    /// When `create` is set, a missing record is created instead of reported.
    pub(crate) fn new(api: A, zone: String, record: String, create: bool) -> Self {
        Self {
            api,
            zone,
            record,
            create,
        }
    }
}

impl<A: DnsApi> ddrfsh_core::Provider for CloudflareProvider<A> {
    type Error = ProviderError;

    fn update(&self, ip: IpAddr) -> Result<(), Self::Error> {
        let kind = RecordType::for_ip(ip);
        let found = self.api.find_records(&self.zone, &self.record, kind)?;

        // Cloudflare filters server-side, but a wrong name or type must never
        // be rewritten, so verify locally before touching anything.
        let matches: Vec<_> = found
            .into_iter()
            .filter(|record| {
                record.name.eq_ignore_ascii_case(&self.record)
                    && record.kind.eq_ignore_ascii_case(kind.as_str())
            })
            .collect();

        match matches.len() {
            0 if self.create => {
                debug!("creating {} record for {}", kind.as_str(), self.record);
                self.api.create_record(&self.zone, &self.record, kind, ip)?;
            }
            0 => {
                return Err(ProviderError::RecordNotFound {
                    zone: self.zone.clone(),
                    name: self.record.clone(),
                    kind: kind.as_str(),
                    hint: "; pass --create to create it",
                });
            }
            1 => {
                let record = &matches[0];
                debug!(
                    "{} record {} currently points at {}",
                    kind.as_str(),
                    record.name,
                    record.content
                );
                self.api.update_record(&self.zone, &record.id, ip)?;
            }
            count => {
                return Err(ProviderError::AmbiguousRecords {
                    zone: self.zone.clone(),
                    name: self.record.clone(),
                    kind: kind.as_str(),
                    count,
                });
            }
        }

        Ok(())
    }
}

/// Errors reported by the Cloudflare provider.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ProviderError {
    /// No record matched the configured name and type.
    #[error("no {kind} record named {name} in zone {zone}{hint}")]
    RecordNotFound {
        zone: String,
        name: String,
        kind: &'static str,
        hint: &'static str,
    },

    /// More than one record matched, so the target is ambiguous.
    #[error("found {count} {kind} records named {name} in zone {zone}; refusing to choose one")]
    AmbiguousRecords {
        zone: String,
        name: String,
        kind: &'static str,
        count: usize,
    },

    /// The API call failed.
    #[error(transparent)]
    Api(#[from] ApiError),
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;

    use super::*;

    use crate::api::Record;

    const ZONE: &str = "0123456789abcdef0123456789abcdef";
    const NAME: &str = "home.example.com";

    #[derive(Default)]
    struct FakeApi {
        found: RefCell<VecDeque<Result<Vec<Record>, ApiError>>>,
        updates: RefCell<Vec<(String, IpAddr)>>,
        creations: RefCell<Vec<(String, RecordType, IpAddr)>>,
    }

    impl FakeApi {
        fn returning(records: Vec<Record>) -> Self {
            let api = Self::default();
            api.found.borrow_mut().push_back(Ok(records));
            api
        }

        fn failing(error: ApiError) -> Self {
            let api = Self::default();
            api.found.borrow_mut().push_back(Err(error));
            api
        }

        fn updates(&self) -> Vec<(String, IpAddr)> {
            self.updates.borrow().clone()
        }

        fn creations(&self) -> Vec<(String, RecordType, IpAddr)> {
            self.creations.borrow().clone()
        }
    }

    impl DnsApi for FakeApi {
        fn find_records(
            &self,
            _zone: &str,
            _name: &str,
            _kind: RecordType,
        ) -> Result<Vec<Record>, ApiError> {
            self.found
                .borrow_mut()
                .pop_front()
                .expect("FakeApi ran out of responses")
        }

        fn update_record(&self, _zone: &str, id: &str, ip: IpAddr) -> Result<(), ApiError> {
            self.updates.borrow_mut().push((id.to_owned(), ip));
            Ok(())
        }

        fn create_record(
            &self,
            _zone: &str,
            _name: &str,
            kind: RecordType,
            ip: IpAddr,
        ) -> Result<(), ApiError> {
            self.creations
                .borrow_mut()
                .push((NAME.to_owned(), kind, ip));
            Ok(())
        }
    }

    fn record(id: &str, name: &str, kind: &str) -> Record {
        Record {
            id: id.to_owned(),
            name: name.to_owned(),
            kind: kind.to_owned(),
            content: "1.1.1.1".to_owned(),
        }
    }

    fn v4() -> IpAddr {
        "8.8.8.8".parse().unwrap()
    }

    fn v6() -> IpAddr {
        "2001:4860:4860::8888".parse().unwrap()
    }

    fn provider(api: FakeApi, create: bool) -> CloudflareProvider<FakeApi> {
        CloudflareProvider::new(api, ZONE.to_owned(), NAME.to_owned(), create)
    }

    #[test]
    fn updates_the_single_matching_record() {
        let api = FakeApi::returning(vec![record("abc", NAME, "A")]);
        let provider = provider(api, false);

        ddrfsh_core::Provider::update(&provider, v4()).unwrap();

        assert_eq!(provider.api.updates(), vec![("abc".to_owned(), v4())]);
        assert!(provider.api.creations().is_empty());
    }

    #[test]
    fn uses_the_aaaa_type_for_ipv6() {
        let api = FakeApi::returning(vec![record("abc", NAME, "AAAA")]);
        let provider = provider(api, false);

        ddrfsh_core::Provider::update(&provider, v6()).unwrap();

        assert_eq!(provider.api.updates(), vec![("abc".to_owned(), v6())]);
    }

    #[test]
    fn ignores_records_that_do_not_match_the_target() {
        let api = FakeApi::returning(vec![
            record("other", "other.example.com", "A"),
            record("wrong-kind", NAME, "AAAA"),
        ]);
        let provider = provider(api, false);

        let err = ddrfsh_core::Provider::update(&provider, v4()).unwrap_err();

        assert!(matches!(err, ProviderError::RecordNotFound { .. }), "{err}");
        assert!(provider.api.updates().is_empty());
    }

    #[test]
    fn reports_a_missing_record_without_creating_it() {
        let api = FakeApi::returning(vec![]);
        let provider = provider(api, false);

        let err = ddrfsh_core::Provider::update(&provider, v4()).unwrap_err();

        match err {
            ProviderError::RecordNotFound { ref hint, .. } => {
                assert!(hint.contains("--create"), "{hint}");
            }
            other => panic!("unexpected error: {other}"),
        }
        assert!(provider.api.creations().is_empty());
        assert!(provider.api.updates().is_empty());
    }

    #[test]
    fn creates_a_missing_record_when_enabled() {
        let api = FakeApi::returning(vec![]);
        let provider = provider(api, true);

        ddrfsh_core::Provider::update(&provider, v6()).unwrap();

        assert_eq!(
            provider.api.creations(),
            vec![(NAME.to_owned(), RecordType::Aaaa, v6())]
        );
        assert!(provider.api.updates().is_empty());
    }

    #[test]
    fn refuses_ambiguous_matches() {
        let api = FakeApi::returning(vec![record("a", NAME, "A"), record("b", NAME, "A")]);
        let provider = provider(api, true);

        let err = ddrfsh_core::Provider::update(&provider, v4()).unwrap_err();

        match err {
            ProviderError::AmbiguousRecords { count, .. } => assert_eq!(count, 2),
            other => panic!("unexpected error: {other}"),
        }
        assert!(provider.api.updates().is_empty());
        assert!(provider.api.creations().is_empty());
    }

    #[test]
    fn propagates_api_errors() {
        let api = FakeApi::failing(ApiError::Request {
            url: "https://api.cloudflare.com/client/v4/zones/x/dns_records".to_owned(),
            message: "connection refused".to_owned(),
        });
        let provider = provider(api, false);

        let err = ddrfsh_core::Provider::update(&provider, v4()).unwrap_err();

        assert!(matches!(err, ProviderError::Api(_)), "{err}");
    }
}
