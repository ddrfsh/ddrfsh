# AGENTS.md

This file is the coding agent guide for this repository. If a more specific `AGENTS.md` exists in a subdirectory, it may supplement this file as long as it does not conflict with it.

## Project Scope

`ddrfsh` is a DDNS client built on the **KISS** principle. Its goal is to do one thing with minimal dependencies, minimal abstraction, and the smallest possible binaries: point a DNS record at the machine's current public IP.

Design priorities:

- Single responsibility and predictable behavior.
- A simple default path; advanced capabilities are opt-in.
- Prefer explicit code over abstractions added "in case they're useful later".
- Each binary serves exactly one DNS/DDNS provider.

## Core Behavior (Must Preserve)

### 1. Default Mode: Stateless One-Shot Execution

Without `--daemon`, the program does exactly two things and exits:

1. Fetch the machine's public IP through a public API.
2. Call the selected DNS/DDNS provider's API to update the record.

Constraints:

- **Never read or write state files or caches.**
- Do not start background threads, timers, or long-running services.
- Repeated runs must remain idempotent: even if the IP has not changed, the update may run and the process must exit successfully.

### 2. `--daemon`: Optional Stateful Polling Mode

`--daemon` runs the two steps above in a loop within the same process and remembers the last successfully updated public IP:

- On startup (no remembered IP), perform an update.
- On every subsequent poll, fetch the public IP first and compare it with the remembered value.
- If the IP is unchanged, skip the update and only emit a debug log.
- If the IP changed, call the provider API to update the record.
- **Remember the new IP only after a successful update.** Never remember an IP when fetching or updating fails, so the next poll retries.
- Keep the remembered IP in memory; the process must not write it to disk. A daemon restart therefore performs one redundant, idempotent update.
- Make the polling interval configurable (for example, `--interval <seconds>`) and keep the default conservative (300 seconds recommended).
- A failed poll must not terminate the daemon; log the error and continue with the next poll.

No mode writes state to disk. Only `--daemon` keeps the remembered IP, and only in memory.

## Monorepo

See [Monorepo instructions](.github/instructions/monorepo.instructions.md).

## Configuration and Credentials

- Each provider reads only the configuration it needs; avoid a generic configuration framework.
- Prefer environment variables for credentials over command-line arguments. Plaintext arguments end up in shell history and process listings.
- Never expose API tokens, secrets, or similar sensitive data in logs or error messages.
- Fail fast on missing or invalid configuration with a clear, actionable error message.

## Testing

- Cover the key pure logic in `core`: IP response parsing, IP normalization, change detection, and daemon decisions.
- Abstract all network requests behind a trait or injectable function so tests can substitute a fake.
- **Tests must not access the real public internet, real DNS APIs, or real credentials.**
- Provider tests should focus on request construction, signing, and response parsing, using fixed fixtures.
- Daemon tests must not sleep on real intervals or touch the filesystem.

## Build and Verification

Run at least the following before committing:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Use narrower scopes as needed:

```bash
cargo test -p ddrfsh-core
cargo test -p ddrfsh-<provider>
cargo run -p ddrfsh-<provider> -- --help
```

After changing `ddrfsh-core`, run at least `cargo check --workspace` to ensure every provider still compiles.

## Code Style

- Follow default `rustfmt` formatting. Do not reformat unrelated code by hand.
- Use names that express intent directly; avoid meaningless `Manager`, `Service`, or `Handler` suffixes.
- Provide rustdoc for public APIs. Comments should explain "why", not restate the code.
- Prefer small functions and explicit data structures over layers of indirection.
- Do not reserve extension points for hypothetical future needs.
- Update the corresponding crate's `README.md` whenever user-visible CLI behavior, configuration, or environment variables change.

## Security and Privacy

- Never commit real credentials or real domain names to the repository, test fixtures, or logs.
- Do not log complete API responses unless you have confirmed they contain no sensitive information.
- Verify the target record before updating DNS to avoid modifying unrelated domains.
- Do not persist any runtime state to disk.

## Agent Working Rules

- Before changing code, decide whether it belongs in `core` or a provider; never put provider logic in the shared layer.
- Make the smallest complete change that fixes the problem; do not refactor unrelated code along the way.
- Every new behavior needs tests. When fixing a bug, add a test that reproduces it first.
- When behavior or conventions change, update the relevant `README.md` and this file.
- If product behavior is unclear, ask first; do not expand the scope on your own.
