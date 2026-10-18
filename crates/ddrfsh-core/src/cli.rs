use std::time::Duration;

use clap::Args;

use crate::error::ConfigError;
use crate::ip::IpFamily;

/// Polling interval used when `--daemon` is passed without `--interval`.
pub const DEFAULT_INTERVAL_SECS: u64 = 300;

/// Smallest accepted polling interval, in seconds.
pub const MIN_INTERVAL_SECS: u64 = 1;

/// Command-line options shared by every provider binary.
#[derive(Debug, Args)]
pub struct CommonArgs {
    /// Run continuously instead of performing a single update.
    #[arg(long)]
    pub daemon: bool,

    /// Polling interval in seconds (daemon mode only).
    #[arg(long, value_name = "SECONDS")]
    pub interval: Option<u64>,

    /// Address family to discover and update.
    #[arg(long, value_enum, default_value = "ipv4")]
    pub ip_family: IpFamily,
}

impl CommonArgs {
    /// Resolves `--daemon` / `--interval` into validated options.
    ///
    /// Returns `None` for the stateless one-shot mode.
    pub fn daemon_options(&self) -> Result<Option<DaemonOptions>, ConfigError> {
        if !self.daemon {
            return match self.interval {
                Some(_) => Err(ConfigError::IntervalWithoutDaemon),
                None => Ok(None),
            };
        }

        match self.interval {
            Some(seconds) => DaemonOptions::new(seconds).map(Some),
            None => Ok(Some(DaemonOptions::default())),
        }
    }
}

/// Validated options for the polling loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DaemonOptions {
    interval: Duration,
}

impl DaemonOptions {
    /// Builds options for a polling interval of `seconds` seconds.
    pub fn new(seconds: u64) -> Result<Self, ConfigError> {
        if seconds < MIN_INTERVAL_SECS {
            return Err(ConfigError::IntervalTooSmall(seconds));
        }
        Ok(Self {
            interval: Duration::from_secs(seconds),
        })
    }

    /// Delay between the start of consecutive polls.
    pub fn interval(self) -> Duration {
        self.interval
    }
}

impl Default for DaemonOptions {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(DEFAULT_INTERVAL_SECS),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use clap::Parser;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(flatten)]
        common: CommonArgs,
    }

    fn parse(args: &[&str]) -> CommonArgs {
        TestCli::parse_from(std::iter::once("ddrfsh-test").chain(args.iter().copied())).common
    }

    #[test]
    fn defaults_to_ipv4_one_shot() {
        let common = parse(&[]);
        assert!(!common.daemon);
        assert_eq!(common.ip_family, IpFamily::Ipv4);
        assert!(common.daemon_options().unwrap().is_none());
    }

    #[test]
    fn accepts_ipv6() {
        assert_eq!(parse(&["--ip-family", "ipv6"]).ip_family, IpFamily::Ipv6);
    }

    #[test]
    fn rejects_auto_and_unknown_families() {
        for value in ["auto", "both", "v4"] {
            let result = TestCli::try_parse_from(["ddrfsh-test", "--ip-family", value]);
            assert!(result.is_err(), "{value} should be rejected");
        }
    }

    #[test]
    fn daemon_defaults_to_five_minutes() {
        let options = parse(&["--daemon"]).daemon_options().unwrap().unwrap();
        assert_eq!(options.interval(), Duration::from_secs(300));
    }

    #[test]
    fn daemon_uses_the_requested_interval() {
        let options = parse(&["--daemon", "--interval", "60"])
            .daemon_options()
            .unwrap()
            .unwrap();
        assert_eq!(options.interval(), Duration::from_secs(60));
    }

    #[test]
    fn rejects_interval_without_daemon() {
        let err = parse(&["--interval", "60"]).daemon_options().unwrap_err();
        assert!(matches!(err, ConfigError::IntervalWithoutDaemon));
    }

    #[test]
    fn rejects_zero_interval() {
        let err = parse(&["--daemon", "--interval", "0"])
            .daemon_options()
            .unwrap_err();
        assert!(matches!(err, ConfigError::IntervalTooSmall(0)));
    }
}
