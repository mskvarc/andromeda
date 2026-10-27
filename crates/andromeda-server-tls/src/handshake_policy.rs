//! Turning loaded material into the rustls configuration one listener serves.

use crate::{
    crypto_provider::broker_crypto_provider,
    pem_material::{ClientVerification, TlsMaterial},
};
use andromeda_configuration::schema::tls::MinimumProtocolVersion;
use rustls::{
    ServerConfig,
    SupportedProtocolVersion,
    server::{VerifierBuilderError, WebPkiClientVerifier},
    sign::SingleCertAndKey,
    version::{TLS12, TLS13},
};
use std::sync::Arc;
use thiserror::Error;

/// Which TLS versions a listener may negotiate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolFloor {
    /// The operator's `tls.minimum_protocol_version` decides.
    Configured,
    /// TLS 1.3 only, whatever is configured: QUIC carries no other version
    /// (RFC 9001 clause 4.2).
    Tls13Only,
}

/// What one listener asks of the handshake: the application protocols it
/// speaks and the oldest TLS version it accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandshakePolicy {
    /// ALPN protocol identifiers in server preference order.
    pub alpn: &'static [&'static [u8]],
    /// The oldest TLS version the listener negotiates.
    pub protocol_floor: ProtocolFloor,
}

/// A rustls configuration that cannot be built from loaded material.
#[derive(Debug, Error)]
pub enum HandshakeConfigError {
    /// The crypto provider rejected the requested protocol versions.
    #[error("cannot enable the requested TLS protocol versions: {0}")]
    ProtocolVersions(#[source] rustls::Error),
    /// The client certificate verifier could not be built from the CA bundle.
    #[error("cannot build the client certificate verifier: {0}")]
    ClientVerifier(#[from] VerifierBuilderError),
}

/// TLS 1.3 and TLS 1.2, in rustls preference order.
static TLS13_AND_TLS12: [&SupportedProtocolVersion; 2] = [&TLS13, &TLS12];

/// TLS 1.3 alone.
static TLS13_ONLY: [&SupportedProtocolVersion; 1] = [&TLS13];

/// The TLS versions a policy admits under the configured floor.
const fn protocol_versions(floor: ProtocolFloor, configured: MinimumProtocolVersion) -> &'static [&'static SupportedProtocolVersion] {
    match (floor, configured) {
        (ProtocolFloor::Configured, MinimumProtocolVersion::Tls12) => &TLS13_AND_TLS12,
        (ProtocolFloor::Configured, MinimumProtocolVersion::Tls13)
        | (ProtocolFloor::Tls13Only, MinimumProtocolVersion::Tls12 | MinimumProtocolVersion::Tls13) => &TLS13_ONLY,
    }
}

/// Build the rustls server configuration `policy` asks for from `material`.
///
/// The configuration presents the material's chain, requests client
/// certificates as the material's [`ClientVerification`] says, negotiates the
/// versions the floor admits and advertises the policy's ALPN tokens.
///
/// # Errors
///
/// Returns [`HandshakeConfigError::ProtocolVersions`] when the provider
/// rejects the versions, and [`HandshakeConfigError::ClientVerifier`] when the
/// client certificate verifier cannot be built.
pub fn build_server_config(material: &TlsMaterial, configured: MinimumProtocolVersion, policy: HandshakePolicy) -> Result<ServerConfig, HandshakeConfigError> {
    let provider = Arc::new(broker_crypto_provider());
    let builder = ServerConfig::builder_with_provider(Arc::clone(&provider))
        .with_protocol_versions(protocol_versions(policy.protocol_floor, configured))
        .map_err(HandshakeConfigError::ProtocolVersions)?;
    let builder = match material.client_verification() {
        ClientVerification::Anonymous => builder.with_no_client_auth(),
        ClientVerification::Optional(roots) => builder.with_client_cert_verifier(
            WebPkiClientVerifier::builder_with_provider(Arc::clone(roots), provider)
                .allow_unauthenticated()
                .build()?,
        ),
        ClientVerification::Required(roots) => {
            builder.with_client_cert_verifier(WebPkiClientVerifier::builder_with_provider(Arc::clone(roots), provider).build()?)
        }
    };
    let mut config = builder.with_cert_resolver(Arc::new(SingleCertAndKey::from(material.certified_key())));
    config.alpn_protocols = policy.alpn.iter().map(|protocol| protocol.to_vec()).collect();
    Ok(config)
}

#[cfg(test)]
mod tests {
    use crate::{
        handshake_policy::{HandshakePolicy, ProtocolFloor, build_server_config},
        handshake_probe::handshake,
        pem_material::TlsMaterial,
        test_identity::{authority::TestAuthority, identity_files::IdentityFiles},
    };
    use andromeda_configuration::schema::tls::{MinimumProtocolVersion, TlsConfig};
    use rustls::{
        ProtocolVersion,
        RootCertStore,
        ServerConfig,
        version::{TLS12, TLS13},
    };
    use std::{error::Error, sync::Arc};

    /// The TCP listener's policy: HTTP/2 preferred over HTTP/1.1.
    const TCP_POLICY: HandshakePolicy = HandshakePolicy {
        alpn: &[b"h2", b"http/1.1"],
        protocol_floor: ProtocolFloor::Configured,
    };

    /// A server configuration for a fresh identity, with a root store trusting it.
    fn server_config(configured: MinimumProtocolVersion, policy: HandshakePolicy) -> Result<(Arc<ServerConfig>, RootCertStore), Box<dyn Error + Send + Sync>> {
        let authority = TestAuthority::generate()?;
        let files = IdentityFiles::write(&authority.issue_server_identity()?)?;
        let tls = TlsConfig {
            certificate: Some(files.certificate_path().to_path_buf()),
            private_key: Some(files.private_key_path().to_path_buf()),
            ..TlsConfig::default()
        };
        let config = build_server_config(&TlsMaterial::load(&tls.material_sources()?)?, configured, policy)?;
        Ok((Arc::new(config), authority.root_store()?))
    }

    #[test]
    fn the_tcp_policy_advertises_h2_before_http11() -> Result<(), Box<dyn Error + Send + Sync>> {
        let (config, _) = server_config(MinimumProtocolVersion::Tls12, TCP_POLICY)?;
        pretty_assertions::assert_eq!(config.alpn_protocols, vec![b"h2".to_vec(), b"http/1.1".to_vec()]);
        Ok(())
    }

    /// Clause 6.2 requires TLS 1.2 support; the default floor admits it and TLS 1.3.
    #[test]
    fn a_tls12_floor_enables_tls12_and_tls13() -> Result<(), Box<dyn Error + Send + Sync>> {
        let (config, roots) = server_config(MinimumProtocolVersion::Tls12, TCP_POLICY)?;
        pretty_assertions::assert_eq!(
            handshake(Arc::clone(&config), roots.clone(), &[&TLS12])?.version,
            Some(ProtocolVersion::TLSv1_2)
        );
        pretty_assertions::assert_eq!(handshake(config, roots, &[&TLS13])?.version, Some(ProtocolVersion::TLSv1_3));
        Ok(())
    }

    #[test]
    fn a_tls13_floor_disables_tls12() -> Result<(), Box<dyn Error + Send + Sync>> {
        let (config, roots) = server_config(MinimumProtocolVersion::Tls13, TCP_POLICY)?;
        assert!(handshake(Arc::clone(&config), roots.clone(), &[&TLS12]).is_err(), "a TLS 1.2 client is refused");
        pretty_assertions::assert_eq!(handshake(config, roots, &[&TLS13])?.version, Some(ProtocolVersion::TLSv1_3));
        Ok(())
    }

    #[test]
    fn the_tls13_only_floor_ignores_a_configured_tls12_floor() -> Result<(), Box<dyn Error + Send + Sync>> {
        let policy = HandshakePolicy {
            alpn: &[b"h3"],
            protocol_floor: ProtocolFloor::Tls13Only,
        };
        let (config, roots) = server_config(MinimumProtocolVersion::Tls12, policy)?;
        assert!(handshake(Arc::clone(&config), roots.clone(), &[&TLS12]).is_err(), "a TLS 1.2 client is refused");
        pretty_assertions::assert_eq!(handshake(config, roots, &[&TLS13])?.version, Some(ProtocolVersion::TLSv1_3));
        Ok(())
    }
}
