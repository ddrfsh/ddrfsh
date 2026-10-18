/// Initializes logging from `RUST_LOG`, defaulting to `info`.
///
/// Safe to call more than once; later calls are ignored.
pub fn init_logging() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_idempotent() {
        init_logging();
        init_logging();
    }
}
