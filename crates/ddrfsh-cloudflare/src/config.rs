use std::env;

/// Environment variable holding the Cloudflare API token.
pub(crate) const TOKEN_VAR: &str = "CLOUDFLARE_API_TOKEN";

/// Environment variable holding the Cloudflare zone ID.
pub(crate) const ZONE_ID_VAR: &str = "CLOUDFLARE_ZONE_ID";

/// Environment variable holding the full record name to update.
pub(crate) const RECORD_VAR: &str = "CLOUDFLARE_RECORD";

const ZONE_ID_LEN: usize = 32;
const MAX_RECORD_LEN: usize = 253;
const MAX_LABEL_LEN: usize = 63;

/// Validated Cloudflare configuration read from the environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CloudflareConfig {
    pub(crate) token: String,
    pub(crate) zone_id: String,
    pub(crate) record: String,
}

impl CloudflareConfig {
    /// Reads and validates the configuration from the process environment.
    pub(crate) fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|name| env::var(name).ok())
    }

    fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let token = required(&lookup, TOKEN_VAR)?;
        let zone_id = validate_zone_id(&required(&lookup, ZONE_ID_VAR)?)?;
        let record = validate_record(&required(&lookup, RECORD_VAR)?)?;

        Ok(Self {
            token,
            zone_id,
            record,
        })
    }
}

/// Configuration errors that are detected before any request is made.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ConfigError {
    /// A required environment variable is absent.
    #[error("{0} must be set")]
    MissingVar(&'static str),

    /// A required environment variable is empty or whitespace only.
    #[error("{0} must not be empty")]
    EmptyVar(&'static str),

    /// The zone ID has the wrong shape.
    #[error("CLOUDFLARE_ZONE_ID must be a 32-character hexadecimal zone ID")]
    InvalidZoneId,

    /// The record name is not a usable DNS host name.
    #[error("CLOUDFLARE_RECORD is invalid: {reason}")]
    InvalidRecord {
        /// Why the name was rejected.
        reason: &'static str,
    },
}

fn required(
    lookup: &impl Fn(&str) -> Option<String>,
    name: &'static str,
) -> Result<String, ConfigError> {
    match lookup(name) {
        None => Err(ConfigError::MissingVar(name)),
        Some(value) => {
            let value = value.trim().to_owned();
            if value.is_empty() {
                Err(ConfigError::EmptyVar(name))
            } else {
                Ok(value)
            }
        }
    }
}

fn validate_zone_id(raw: &str) -> Result<String, ConfigError> {
    if raw.len() != ZONE_ID_LEN || !raw.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ConfigError::InvalidZoneId);
    }
    Ok(raw.to_ascii_lowercase())
}

fn validate_record(raw: &str) -> Result<String, ConfigError> {
    if !raw.is_ascii() {
        return Err(ConfigError::InvalidRecord {
            reason: "use a Punycode name instead of non-ASCII characters",
        });
    }
    if raw.ends_with('.') {
        return Err(ConfigError::InvalidRecord {
            reason: "remove the trailing dot; pass a fully qualified name without it",
        });
    }
    if raw.contains('*') {
        return Err(ConfigError::InvalidRecord {
            reason: "wildcard records are not supported",
        });
    }
    if raw.len() > MAX_RECORD_LEN {
        return Err(ConfigError::InvalidRecord {
            reason: "the name is longer than 253 characters",
        });
    }
    if raw.split('.').any(|label| !is_valid_label(label)) {
        return Err(ConfigError::InvalidRecord {
            reason: "each dot-separated label must be 1-63 characters of letters, digits, or hyphens and must not start or end with a hyphen",
        });
    }

    Ok(raw.to_ascii_lowercase())
}

fn is_valid_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= MAX_LABEL_LEN
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    const ZONE: &str = "0123456789abcdef0123456789abcdef";

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        move |name| map.get(name).cloned()
    }

    fn valid() -> Vec<(&'static str, &'static str)> {
        vec![
            (TOKEN_VAR, "token"),
            (ZONE_ID_VAR, ZONE),
            (RECORD_VAR, "home.example.com"),
        ]
    }

    fn parse(pairs: &[(&str, &str)]) -> Result<CloudflareConfig, ConfigError> {
        CloudflareConfig::from_lookup(lookup(pairs))
    }

    #[test]
    fn reads_a_complete_configuration() {
        let config = parse(&valid()).unwrap();
        assert_eq!(config.token, "token");
        assert_eq!(config.zone_id, ZONE);
        assert_eq!(config.record, "home.example.com");
    }

    #[test]
    fn trims_surrounding_whitespace() {
        let config = parse(&[
            (TOKEN_VAR, "  token\n"),
            (ZONE_ID_VAR, ZONE),
            (RECORD_VAR, " home.example.com "),
        ])
        .unwrap();
        assert_eq!(config.token, "token");
        assert_eq!(config.record, "home.example.com");
    }

    #[test]
    fn lowercases_zone_and_record() {
        let config = parse(&[
            (TOKEN_VAR, "token"),
            (ZONE_ID_VAR, "0123456789ABCDEF0123456789ABCDEF"),
            (RECORD_VAR, "Home.Example.COM"),
        ])
        .unwrap();
        assert_eq!(config.zone_id, ZONE);
        assert_eq!(config.record, "home.example.com");
    }

    #[test]
    fn reports_missing_variables() {
        for missing in [TOKEN_VAR, ZONE_ID_VAR, RECORD_VAR] {
            let pairs: Vec<_> = valid()
                .into_iter()
                .filter(|(key, _)| *key != missing)
                .collect();
            let err = parse(&pairs).unwrap_err();
            assert!(
                matches!(err, ConfigError::MissingVar(name) if name == missing),
                "{missing}: {err}"
            );
        }
    }

    #[test]
    fn reports_empty_variables() {
        for empty in [TOKEN_VAR, ZONE_ID_VAR, RECORD_VAR] {
            let pairs: Vec<_> = valid()
                .into_iter()
                .map(|(key, value)| {
                    if key == empty {
                        (key, "   ")
                    } else {
                        (key, value)
                    }
                })
                .collect();
            let err = parse(&pairs).unwrap_err();
            assert!(
                matches!(err, ConfigError::EmptyVar(name) if name == empty),
                "{empty}: {err}"
            );
        }
    }

    #[test]
    fn rejects_invalid_zone_ids() {
        for zone in [
            "0123",
            "0123456789abcdef0123456789abcde",
            "0123456789abcdef0123456789abcdef0",
            "0123456789abcdef0123456789abcdeg",
            "0123456789abcdef 123456789abcdef",
        ] {
            let mut pairs = valid();
            pairs[1] = (ZONE_ID_VAR, zone);
            let result = parse(&pairs);
            assert!(
                matches!(result, Err(ConfigError::InvalidZoneId)),
                "{zone:?}: {result:?}"
            );
        }
    }

    #[test]
    fn rejects_invalid_record_names() {
        for name in [
            "home..example.com",
            ".example.com",
            "example.com.",
            "*.example.com",
            "-home.example.com",
            "home-.example.com",
            "home example.com",
            "例子.example.com",
        ] {
            let mut pairs = valid();
            pairs[2] = (RECORD_VAR, name);
            let result = parse(&pairs);
            assert!(
                matches!(result, Err(ConfigError::InvalidRecord { .. })),
                "{name:?}: {result:?}"
            );
        }
    }

    #[test]
    fn accepts_apex_and_single_label_records() {
        for name in ["example.com", "localhost", "a.b.c.example.com"] {
            let mut pairs = valid();
            pairs[2] = (RECORD_VAR, name);
            assert_eq!(parse(&pairs).unwrap().record, name, "{name}");
        }
    }
}
