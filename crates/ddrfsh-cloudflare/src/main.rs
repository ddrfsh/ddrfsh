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

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Update a Cloudflare DNS record with this machine's public IP"
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
