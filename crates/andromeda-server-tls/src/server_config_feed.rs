//! The live rustls configuration one listener hands to each new handshake.

use crate::{
    config::MinimumProtocolVersion,
    handshake_policy::{HandshakeConfigError, HandshakePolicy, build_server_config},
    material_watch::TlsMaterialWatch,
    pem_material::TlsMaterial,
};
use rustls::ServerConfig;
use std::sync::Arc;
use tokio::sync::watch;

/// The rustls configuration a listener serves, rebuilt whenever the broker's
/// material changes.
///
/// A listener reads [`current`](ServerConfigFeed::current) once per handshake,
/// so a rotated certificate is presented to the next connection while
/// connections already open keep the session they negotiated.
#[derive(Debug, Clone)]
pub struct ServerConfigFeed {
    /// The most recently built configuration.
    receiver: watch::Receiver<Arc<ServerConfig>>,
}

impl ServerConfigFeed {
    /// The configuration the next handshake should use.
    #[must_use]
    pub fn current(&self) -> Arc<ServerConfig> {
        Arc::clone(&self.receiver.borrow())
    }

    /// Wait for the next rebuilt configuration.
    ///
    /// Resolves to `None` once the configuration can no longer change, because
    /// the [`TlsMaterialWatch`] it was derived from has been dropped.
    pub async fn changed(&mut self) -> Option<Arc<ServerConfig>> {
        self.receiver.changed().await.ok()?;
        Some(Arc::clone(&self.receiver.borrow_and_update()))
    }
}

impl TlsMaterialWatch {
    /// Derive the live configuration a listener with `policy` serves.
    ///
    /// The first configuration is built immediately, so an unusable policy
    /// fails startup; later ones are rebuilt by a task spawned on the ambient
    /// Tokio runtime, which must therefore be running. A rebuild that fails is
    /// logged and the previous configuration keeps being served.
    ///
    /// # Errors
    ///
    /// Returns a [`HandshakeConfigError`] when the first configuration cannot be
    /// built from the current material.
    pub fn feed(&self, policy: HandshakePolicy) -> Result<ServerConfigFeed, HandshakeConfigError> {
        let mut material = self.subscribe();
        let current = Arc::clone(&material.borrow_and_update());
        let first = build_server_config(&current, self.minimum_protocol_version(), policy)?;
        let (sender, receiver) = watch::channel(Arc::new(first));
        tokio::spawn(rebuild_on_change(material, sender, self.minimum_protocol_version(), policy));
        Ok(ServerConfigFeed { receiver })
    }
}

/// Rebuild the configuration on every material change until either the
/// material watch or every holder of the feed is gone.
async fn rebuild_on_change(
    mut material: watch::Receiver<Arc<TlsMaterial>>,
    sender: watch::Sender<Arc<ServerConfig>>,
    configured: MinimumProtocolVersion,
    policy: HandshakePolicy,
) {
    loop {
        tokio::select! {
            changed = material.changed() => {
                if changed.is_err() {
                    return;
                }
            }
            () = sender.closed() => return,
        }
        let current = Arc::clone(&material.borrow_and_update());
        match build_server_config(&current, configured, policy) {
            Ok(config) => {
                sender.send_replace(Arc::new(config));
            }
            Err(error) => tracing::error!(
                event = "tls_server_config_rebuild_rejected",
                error = %error,
                "keeping the previous TLS server configuration"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        config::TlsConfig,
        handshake_policy::{HandshakePolicy, ProtocolFloor},
        handshake_probe::handshake,
        material_watch::TlsMaterialWatch,
    };
    use andromeda_configuration_scalars::human_duration::HumanDuration;
    use andromeda_test_tls::{authority::TestAuthority, identity_files::IdentityFiles};
    use rustls::version::TLS13;
    use std::{error::Error, sync::Arc, time::Duration};
    use tokio::time::timeout;

    #[tokio::test]
    async fn a_feed_rebuilds_after_material_changes() -> Result<(), Box<dyn Error + Send + Sync>> {
        let authority = TestAuthority::generate()?;
        let first = authority.issue_server_identity()?;
        let second = authority.issue_server_identity()?;
        let files = IdentityFiles::write(&first)?;
        let config = TlsConfig {
            certificate_path: files.certificate_path().to_path_buf(),
            private_key_path: files.private_key_path().to_path_buf(),
            reload_debounce: HumanDuration::from_millis(100),
            ..TlsConfig::default()
        };
        let watch = TlsMaterialWatch::start(&config)?;
        let mut feed = watch.feed(HandshakePolicy {
            alpn: &[b"http/1.1"],
            protocol_floor: ProtocolFloor::Configured,
        })?;
        let before = feed.current();
        pretty_assertions::assert_eq!(
            handshake(Arc::clone(&before), authority.root_store()?, &[&TLS13])?.leaf.as_ref(),
            Some(first.certificate())
        );
        files.replace(&second)?;
        let rebuilt = timeout(Duration::from_secs(5), feed.changed()).await?.ok_or("the feed closed")?;
        assert!(!Arc::ptr_eq(&before, &rebuilt), "a new configuration is published");
        assert!(Arc::ptr_eq(&rebuilt, &feed.current()), "the rebuilt configuration is current");
        pretty_assertions::assert_eq!(rebuilt.alpn_protocols, vec![b"http/1.1".to_vec()]);
        pretty_assertions::assert_eq!(
            handshake(rebuilt, authority.root_store()?, &[&TLS13])?.leaf.as_ref(),
            Some(second.certificate())
        );
        Ok(())
    }
}
