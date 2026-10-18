# ddrfsh-core

Shared library for `ddrfsh` provider binaries.

It is not a user-facing tool. Provider crates depend on it to reuse the
common DDNS flow, and it contains no provider-specific logic.

## What it provides

- Public IP discovery over HTTPS (`HttpIpSource`, `parse_public_ip`).
- The `Provider` trait that each provider crate implements.
- `run_once`: stateless one-shot execution.
- `run_daemon`: polling loop that remembers the last updated IP in memory.
- `CommonArgs`: the `--daemon`, `--interval`, and `--ip-family` options.

## Adding a provider binary

```rust
// src/main.rs
use std::net::IpAddr;

use clap::Parser;
use ddrfsh_core::{
    init_logging, run_daemon, run_once, CommonArgs, HttpIpSource, Provider,
};

#[derive(Debug, Parser)]
struct Cli {
    #[command(flatten)]
    common: CommonArgs,
}

struct MyProvider;

impl Provider for MyProvider {
    type Error = MyError;

    fn update(&self, ip: IpAddr) -> Result<(), Self::Error> {
        // Update the A record for `IpAddr::V4` and the AAAA record
        // for `IpAddr::V6`, then return the provider's result.
        let _ = ip;
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();
    let cli = Cli::parse();
    let source = HttpIpSource::for_family(cli.common.ip_family);
    let provider = MyProvider;

    match cli.common.daemon_options()? {
        Some(options) => run_daemon(&source, &provider, options),
        None => {
            run_once(&source, &provider)?;
            Ok(())
        }
    }
}
```

## Behaviour contract

- One-shot mode never reads or writes state.
- Daemon mode keeps the last successfully updated IP in memory only; it is
  never written to disk, so a restart performs one redundant, idempotent
  update.
- A failed poll never terminates the daemon.
- `--ip-family ipv4` (default) updates an A record, `--ip-family ipv6`
  updates an AAAA record. The family is never switched automatically.
