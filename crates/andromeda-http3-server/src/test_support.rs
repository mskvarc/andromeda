//! TLS material shared across tests.
//!
//! HTTP/3 cannot be exercised without a certificate, so every test that binds
//! or dials a QUIC endpoint needs a throwaway identity served through the same
//! shared-identity feed the broker uses; setting it up once here keeps that out
//! of the individual test modules. [`QuicIdentity::dial`] opens an
//! [`Http3Client`] on such a listener, so the server modules are tested over
//! the wire rather than through mocks.

use crate::{
    http3_client::Http3Client,
    quic_crypto::{H3_ALPN, QUIC_HANDSHAKE_POLICY},
};
use andromeda_configuration::{
    human_duration::HumanDuration,
    schema::tls::{ClientAuthenticationConfig, ClientAuthenticationMode, TlsConfig},
};
use andromeda_server_tls::{
    material_watch::TlsMaterialWatch,
    server_config_feed::ServerConfigFeed,
    test_identity::{authority::TestAuthority, identity::TestIdentity, identity_files::IdentityFiles},
};
use rustls::RootCertStore;
use std::{error::Error, net::SocketAddr};

/// A server identity on disk, watched and fed to the QUIC endpoint exactly as
/// the broker feeds its shared identity, with the authority that issued it
/// kept for client trust.
pub(crate) struct QuicIdentity {
    /// The live TLS configuration the endpoint serves.
    feed: ServerConfigFeed,
    /// Keeps the identity files watched for the listener's lifetime.
    _watch: TlsMaterialWatch,
    /// The identity files, replaceable to rotate the certificate.
    files: IdentityFiles,
    /// The authority that issued the server's certificate.
    authority: TestAuthority,
}

impl QuicIdentity {
    /// An identity requesting no client certificate.
    ///
    /// Must be called from within a Tokio runtime, which rebuilds the feed.
    pub(crate) fn anonymous() -> Result<QuicIdentity, Box<dyn Error + Send + Sync>> {
        QuicIdentity::start(ClientAuthenticationMode::Disabled, None)
    }

    /// An identity requiring a client certificate issued by `clients`.
    pub(crate) fn requiring_clients_of(clients: &TestAuthority) -> Result<QuicIdentity, Box<dyn Error + Send + Sync>> {
        QuicIdentity::start(ClientAuthenticationMode::Required, Some(clients))
    }

    /// Write a fresh `localhost` identity and feed it under the QUIC policy.
    fn start(client_authentication: ClientAuthenticationMode, clients: Option<&TestAuthority>) -> Result<QuicIdentity, Box<dyn Error + Send + Sync>> {
        let authority = TestAuthority::generate()?;
        let files = IdentityFiles::write(&authority.issue_server_identity()?)?;
        let ca = clients.map(|issuer| files.write_client_ca(issuer)).transpose()?;
        let config = TlsConfig {
            certificate: Some(files.certificate_path().to_path_buf()),
            private_key: Some(files.private_key_path().to_path_buf()),
            client_authentication: ClientAuthenticationConfig {
                mode: client_authentication,
                ca,
            },
            reload_settle_after: HumanDuration::from_millis(100),
            ..TlsConfig::default()
        };
        let watch = TlsMaterialWatch::start(&config)?;
        let feed = watch.feed(QUIC_HANDSHAKE_POLICY)?;
        Ok(QuicIdentity {
            feed,
            _watch: watch,
            files,
            authority,
        })
    }

    /// The live configuration the endpoint serves.
    pub(crate) const fn feed(&self) -> &ServerConfigFeed {
        &self.feed
    }

    /// A root store trusting only this identity's authority.
    pub(crate) fn root_store(&self) -> Result<RootCertStore, Box<dyn Error + Send + Sync>> {
        Ok(self.authority.root_store()?)
    }

    /// Replace the identity files with a newly issued leaf and return it.
    pub(crate) fn rotate(&self) -> Result<TestIdentity, Box<dyn Error + Send + Sync>> {
        let rotated = self.authority.issue_server_identity()?;
        self.files.replace(&rotated)?;
        Ok(rotated)
    }

    /// Dial the listener over HTTP/3, trusting only this identity's authority.
    pub(crate) async fn dial(&self, server: SocketAddr) -> Result<Http3Client, Box<dyn Error + Send + Sync>> {
        Ok(Http3Client::connect(self.root_store()?, vec![H3_ALPN.to_vec()], server, None).await?)
    }
}
