//! TLS configuration for direct reqwest HTTPS clients.
//!
//! reqwest 0.13 defaults its `rustls` feature to `rustls-platform-verifier`
//! (the OS trust store). Our devices are headless and may not have a populated
//! OS trust store, so we pin clients to the bundled webpki CA roots instead.

/// Build a rustls `ClientConfig` that trusts only the bundled webpki roots
/// (no OS trust store).
pub(crate) fn webpki_tls_config() -> rustls::ClientConfig {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    // Use the ring provider explicitly rather than the process-default: with
    // `rustls-no-provider` no default provider is installed, and with both
    // ring + aws-lc-rs compiled `builder()` can't pick.
    rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .expect("ring supports the default protocol versions")
    .with_root_certificates(roots)
    .with_no_client_auth()
}
