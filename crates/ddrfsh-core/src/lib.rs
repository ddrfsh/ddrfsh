//! Shared logic for `ddrfsh` DDNS clients.
//!
//! This crate holds the code that is common to every provider binary:
//! public IP discovery, provider abstraction, execution modes, and state.
//! It must never contain provider-specific authentication, signing, or
//! response mapping.
