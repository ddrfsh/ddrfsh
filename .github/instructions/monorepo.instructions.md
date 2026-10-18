---
description: "Cargo workspace layout, crate boundaries, independent versioning, and verification rules for this Rust monorepo"
applyTo: "**/Cargo.toml,crates/**/*.rs"
---

# Monorepo Instructions

## Monorepo Structure

This repository is a Cargo workspace. The root only carries workspace configuration and contains no business logic.

```text
Cargo.toml                    # Virtual manifest, only [workspace] / [workspace.*]
crates/
  ddrfsh-core/                # Shared logic library
  ddrfsh-<provider>/          # One executable crate per provider
```

### Root Directory

- `Cargo.toml` must be a **virtual manifest** (only `[workspace]`, no `[package]`).
- Do not add `src/` or any business code at the root.
- The root manifest owns members, resolver, shared dependency versions, workspace lints, and profiles.
- Repository-level docs, CI, licenses, and configuration files may live at the root.

### `ddrfsh-core`

Only cross-provider shared logic belongs here, for example:

- Public IP discovery (public API calls, response parsing, IP normalization).
- Provider abstraction (update request/result types, the `Provider` trait).
- Orchestration for one-shot execution and the daemon loop.
- State reading and writing.
- Shared error types, logging initialization, and HTTP client wrapper.

Constraints:

- `core` must not depend on any provider crate.
- `core` must not contain provider-specific authentication, request signing, or field mapping.
- Keep the public API small and stable; do not let one provider's requirements pollute shared abstractions.

### `ddrfsh-<provider>`

Each DNS/DDNS provider gets its own crate and produces a binary with the same name:

```text
crates/ddrfsh-cloudflare/   -> ddrfsh-cloudflare
crates/ddrfsh-aliyun/       -> ddrfsh-aliyun
```

Constraints:

- One crate implements exactly one provider.
- Include only that provider's credential parsing, API request construction/signing, response parsing, and any necessary CLI arguments.
- Reuse shared flows by depending on `ddrfsh-core`.
- **Never** let provider crates depend on each other.
- **Never** bundle multiple providers into one binary via Cargo features.
- Do not introduce generics, macros, or trait objects for a scenario with only one implementation; implement the `core` trait directly.
- `main.rs` only parses arguments, wires dependencies, and exits on error. Put business logic in testable modules.

### New Provider Checklist

1. Run `cargo new crates/ddrfsh-<provider> --bin`.
2. Declare an independent version in the crate and add a path dependency on core.
3. Implement the provider update logic and integrate it with core's `Provider` abstraction.
4. Wire up the shared CLI (at minimum, default one-shot mode and `--daemon`).
5. Document credentials, add a minimal configuration example, and add a `README.md`.
6. Add tests that do not access the real network.
7. Ensure `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` pass.

## Versioning and Releases

- **Each crate is versioned independently.** Do not use `version.workspace = true` in a crate.
- Put shared third-party dependency versions in the root `[workspace.dependencies]`; reference them from crates with `workspace = true`.
- Declare `version` alongside `path` for cross-crate path dependencies to support future independent publishing.
- Follow SemVer. When changing the public API of `ddrfsh-core`, compile every provider crate.
- Do not bump unrelated crates as a side effect of one change.
- The release process must support publishing crates individually (for example, `cargo release -p <crate>` or release-plz).

Versioning example:

```toml
# Root Cargo.toml
[workspace.dependencies]
ddrfsh-core = { path = "crates/ddrfsh-core", version = "0.1.0" }
```

```toml
# crates/ddrfsh-cloudflare/Cargo.toml
[package]
name = "ddrfsh-cloudflare"
version = "0.2.0"          # Independent version, not inherited from the workspace

[dependencies]
ddrfsh-core.workspace = true
```

## Workspace Conventions

- Use `edition = "2024"` and `resolver = "3"`. Declare the minimum supported Rust version centrally via `rust-version` in the root manifest.
- Configure lints centrally in the root `[workspace.lints]`; each crate opts in with `[lints] workspace = true`.
- Keep dependencies minimal and prefer the standard library. When adding a dependency, explain the problem it solves and avoid pulling in a large dependency tree for a small feature.
- Default to a blocking, lightweight HTTP client and do not introduce an async runtime. If asynchronous behavior is genuinely required, explain why and centralize the wrapper in core.
- Initialize logging exactly once in the binary entry point or a shared core entry point. Providers must not initialize a global logger themselves.
- Error handling:
  - `core` defines structured error types so callers can distinguish failure reasons.
  - Providers add context at their boundaries.
  - Do not use `unwrap()` / `expect()` for recoverable errors in library code (test code is exempt).
