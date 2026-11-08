//! The TLS client configuration every `mqtts` broker connection is opened with.
//!
//! A broker's certificate is checked by the platform verifier, the trust the
//! HTTP delivery client also applies, so an `mqtts` and an `https` endpoint on
//! one host are accepted or refused alike. The system roots are read once, when
//! the configuration is built; a system certificate that cannot be read or
//! parsed is skipped with a warning instead of failing the build, and only a
//! system with no usable root at all is an error.

use rustls::{ClientConfig, crypto::aws_lc_rs::default_provider};
use rustls_platform_verifier::BuilderVerifierExt;
use std::sync::Arc;

/// Build the TLS client configuration for `mqtts` connections, on the
/// `aws-lc-rs` provider and the protocol versions `rustls` deems safe.
///
/// # Errors
///
/// Returns the [`rustls::Error`] the platform verifier reports when it cannot
/// be built: on Linux and the BSDs, when not one system root certificate loads.
pub fn mqtt_client_tls() -> Result<Arc<ClientConfig>, rustls::Error> {
    let config = ClientConfig::builder_with_provider(Arc::new(default_provider()))
        .with_safe_default_protocol_versions()?
        .with_platform_verifier()?
        .with_no_client_auth();
    Ok(Arc::new(config))
}

#[cfg(test)]
mod tests {
    use crate::mqtt::client_tls::mqtt_client_tls;

    #[test]
    fn the_mqtt_tls_configuration_builds_from_the_system_trust_store() {
        let config = mqtt_client_tls().expect("the system trust store yields a TLS client configuration");
        assert!(config.alpn_protocols.is_empty(), "MQTT over TLS negotiates no application protocol");
    }
}
