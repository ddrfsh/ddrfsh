//! Cloudflare DDNS client.
//!
//! Discovers this machine's public IP through `ddrfsh-core`, then updates the
//! matching A or AAAA record through the Cloudflare v4 API.
//!
//! Configuration comes from `CLOUDFLARE_API_TOKEN`, `CLOUDFLARE_ZONE_ID`, and
//! `CLOUDFLARE_RECORD`; see `README.md` for details.

mod api;
mod config;
mod http_api;
mod provider;

use std::process::ExitCode;

use clap::Parser;
use ddrfsh_core::{CommonArgs, HttpIpSource, init_logging, run_daemon, run_once};

use config::{CloudflareConfig, ConfigError};
use http_api::HttpApi;
use provider::{CloudflareProvider, ProviderError};

const AFTER_HELP: &str = r#"Environment variables:
  CLOUDFLARE_API_TOKEN   Cloudflare API token with Zone.DNS:Edit permission (required)
  CLOUDFLARE_ZONE_ID     32-character hexadecimal zone ID (required)
  CLOUDFLARE_RECORD      Full record name, e.g. home.example.com (required)

The token is read from the environment so it never appears in shell history or
process listings.

Example:
  CLOUDFLARE_API_TOKEN=... CLOUDFLARE_ZONE_ID=... CLOUDFLARE_RECORD=home.example.com \
    ddrfsh-cloudflare --ip-family ipv6 --daemon"#;

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Update a Cloudflare DNS record with this machine's public IP",
    after_help = AFTER_HELP
)]
struct Cli {
    #[command(flatten)]
    common: CommonArgs,

    /// Create the DNS record when it does not exist yet.
    #[arg(long)]
    create: bool,
}

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error(transparent)]
    Config(#[from] ConfigError),

    #[error(transparent)]
    Cli(#[from] ddrfsh_core::ConfigError),

    #[error(transparent)]
    Run(#[from] ddrfsh_core::Error<ProviderError>),
}

fn main() -> ExitCode {
    init_logging();
    let cli = Cli::parse();

    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), Error> {
    let config = CloudflareConfig::from_env()?;
    let api = HttpApi::new(config.token);
    let provider = CloudflareProvider::new(api, config.zone_id, config.record, cli.create);
    let source = HttpIpSource::for_family(cli.common.ip_family);

    match cli.common.daemon_options()? {
        Some(options) => run_daemon(&source, &provider, options),
        None => {
            run_once(&source, &provider)?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn help_documents_the_environment_variables() {
        let help = Cli::command().render_help().to_string();

        for name in [config::TOKEN_VAR, config::ZONE_ID_VAR, config::RECORD_VAR] {
            assert!(help.contains(name), "{name} missing from:\n{help}");
        }
    }

    #[test]
    fn long_help_shows_an_example() {
        let help = Cli::command().render_long_help().to_string();

        assert!(help.contains("--ip-family ipv6"), "{help}");
        assert!(
            help.contains("CLOUDFLARE_RECORD=home.example.com"),
            "{help}"
        );
    }
}
