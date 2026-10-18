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

`--daemon` runs the two steps above in a loop within the same process and tracks the last successfully updated public IP:

- On first run (no state file), perform an update.
- On every subsequent poll, fetch the public IP first and compare it with the recorded value.
- If the IP is unchanged, skip the update and only emit a debug log.
- If the IP changed, call the provider API to update the record.
- **Persist the new IP only after a successful update.** Never write state when fetching the IP or updating fails, so the next poll retries.
- Keep state isolated per provider; the file name must include the provider name.
- Store the state file in the OS user state directory (for example, `$XDG_STATE_HOME` on Linux or `~/Library/Application Support` on macOS). Never write it to the current working directory.
- Make the polling interval configurable (for example, `--interval <seconds>`) and keep the default conservative (300 seconds recommended).
- A failed poll must not terminate the daemon; log the error and continue with the next poll.

Except in `--daemon` mode, no mode may produce persistent state.

## Monorepo

See [Monorepo instructions](.github/instructions/monorepo.instructions.md).

## Configuration and Credentials

- Each provider reads only the configuration it needs; avoid a generic configuration framework.
- Prefer environment variables for credentials over command-line arguments. Plaintext arguments end up in shell history and process listings.
- Never expose API tokens, secrets, or similar sensitive data in logs, error messages, or state files.
- Fail fast on missing or invalid configuration with a clear, actionable error message.

## Testing

- Cover the key pure logic in `core`: IP response parsing, IP normalization, change detection, daemon decisions, and state I/O.
- Abstract all network requests behind a trait or injectable function so tests can substitute a fake.
- **Tests must not access the real public internet, real DNS APIs, or real credentials.**
- Provider tests should focus on request construction, signing, and response parsing, using fixed fixtures.
- State file tests must use temporary directories and must not pollute user directories.

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
- Store only the minimum information required to operate in the state file (at least the last successfully updated public IP and a timestamp; never credentials).

## Agent Working Rules

- Before changing code, decide whether it belongs in `core` or a provider; never put provider logic in the shared layer.
- Make the smallest complete change that fixes the problem; do not refactor unrelated code along the way.
- Every new behavior needs tests. When fixing a bug, add a test that reproduces it first.
- When behavior or conventions change, update the relevant `README.md` and this file.
- If product behavior is unclear, ask first; do not expand the scope on your own.
