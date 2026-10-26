//! Keeping the broker's identity current as its files change on disk.
//!
//! Certificates are rotated without restarting the broker: a certificate
//! manager renews them, a Kubernetes Secret volume swaps them in, an operator
//! copies new ones over the old. The watch loads the material once at start,
//! failing startup if it is unusable, then reloads it whenever the directories
//! holding it change. A reload that fails -- a half-written file, a key that no
//! longer matches its certificate -- is logged and discarded, and the previous
//! material keeps being served: a broken rotation must never take a working
//! listener down.

use crate::pem_material::{MaterialChange, TlsMaterial, TlsMaterialError};
use andromeda_configuration::schema::{
    tls::{MinimumProtocolVersion, TlsConfig, TlsConfigError},
    tls_material_sources::TlsMaterialSources,
};
use notify_debouncer_full::{
    DebounceEventHandler,
    DebounceEventResult,
    Debouncer,
    RecommendedCache,
    new_debouncer,
    notify::{Error as NotifyError, EventKind, RecommendedWatcher, RecursiveMode},
};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::watch;

/// The broker's identity could not be loaded or watched at startup.
#[derive(Debug, Error)]
pub enum TlsWatchError {
    /// The `[tls]` section names no complete identity.
    #[error(transparent)]
    Config(#[from] TlsConfigError),
    /// The configured files do not form a usable identity.
    #[error(transparent)]
    Material(#[from] TlsMaterialError),
    /// The directories holding the files cannot be watched.
    #[error("cannot watch the TLS material directories: {0}")]
    Watch(#[from] NotifyError),
}

/// The broker's current TLS material, reloaded whenever its files change.
///
/// Dropping the watch stops watching; every [`ServerConfigFeed`] derived from
/// it then keeps serving the last configuration it built.
///
/// [`ServerConfigFeed`]: crate::server_config_feed::ServerConfigFeed
pub struct TlsMaterialWatch {
    /// Watches the material directories and reloads on change. Declared first
    /// so it stops, and releases the publishing side of `material`, before the
    /// receiver below is dropped.
    _debouncer: Debouncer<RecommendedWatcher, RecommendedCache>,
    /// The most recently accepted material.
    material: watch::Receiver<Arc<TlsMaterial>>,
    /// The configured floor, applied when a feed is built.
    minimum_protocol_version: MinimumProtocolVersion,
}

impl TlsMaterialWatch {
    /// Load the configured material and start watching it.
    ///
    /// # Errors
    ///
    /// Returns [`TlsWatchError::Config`] when the section names no complete
    /// identity, [`TlsWatchError::Material`] when the files do not form a usable
    /// one, and [`TlsWatchError::Watch`] when their directories cannot be
    /// watched.
    pub fn start(config: &TlsConfig) -> Result<TlsMaterialWatch, TlsWatchError> {
        let sources = config.material_sources()?;
        let initial = TlsMaterial::load(&sources)?;
        let directories = sources.watched_directories();
        let (sender, material) = watch::channel(Arc::new(initial));
        let reloader = MaterialReloader {
            sources: sources.into_owned(),
            sender,
        };
        let mut debouncer = new_debouncer(config.reload_debounce.duration(), None, reloader)?;
        directories
            .iter()
            .try_for_each(|directory| debouncer.watch(directory, RecursiveMode::NonRecursive))?;
        Ok(TlsMaterialWatch {
            _debouncer: debouncer,
            material,
            minimum_protocol_version: config.minimum_protocol_version,
        })
    }

    /// The material currently being served.
    #[must_use]
    pub fn current(&self) -> Arc<TlsMaterial> {
        Arc::clone(&self.material.borrow())
    }

    /// A receiver observing every material change from now on.
    pub(crate) fn subscribe(&self) -> watch::Receiver<Arc<TlsMaterial>> {
        self.material.clone()
    }

    /// The configured oldest TLS version for TCP handshakes.
    #[must_use]
    pub const fn minimum_protocol_version(&self) -> MinimumProtocolVersion {
        self.minimum_protocol_version
    }
}

/// Reloads the material on the watcher's thread and publishes what it accepts.
struct MaterialReloader {
    /// The files the material is loaded from.
    sources: TlsMaterialSources<'static>,
    /// Where accepted material is published.
    sender: watch::Sender<Arc<TlsMaterial>>,
}

impl MaterialReloader {
    /// Load the files again and publish them if they changed and are usable.
    fn reload(&self) {
        let material = match TlsMaterial::load(&self.sources) {
            Ok(material) => material,
            Err(error) => {
                tracing::error!(event = "tls_reload_rejected", error = %error, "keeping the previous TLS material");
                return;
            }
        };
        let change = material.change_from(&self.sender.borrow());
        match change {
            MaterialChange::Unchanged => tracing::debug!(event = "tls_material_unchanged"),
            MaterialChange::Changed => {
                self.sender.send_replace(Arc::new(material));
                tracing::info!(
                    event = "tls_material_reloaded",
                    certificate = %self.sources.certificate_path().display(),
                    "TLS material reloaded; new handshakes present it"
                );
            }
        }
    }
}

impl DebounceEventHandler for MaterialReloader {
    fn handle_event(&mut self, result: DebounceEventResult) {
        match result {
            // Reading a file is not a change to it; anything else in a watched
            // directory may be the rotation, including renames of symlinks the
            // configured paths resolve through.
            Ok(events) => {
                if events.iter().any(|debounced| !matches!(debounced.event.kind, EventKind::Access(_))) {
                    self.reload();
                }
            }
            Err(errors) => errors
                .iter()
                .for_each(|error| tracing::warn!(event = "tls_watch_error", error = %error, "the TLS material watcher reported an error")),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        material_watch::{TlsMaterialWatch, TlsWatchError},
        pem_material::TlsMaterialError,
        test_identity::{authority::TestAuthority, identity::TestIdentity, identity_files::IdentityFiles},
    };
    use andromeda_configuration::{human_duration::HumanDuration, schema::tls::TlsConfig};
    use rustls_pki_types::CertificateDer;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    use std::{
        error::Error,
        fs,
        path::Path,
        thread,
        time::{Duration, Instant},
    };
    use tempfile::TempDir;

    /// How long a test waits for a rotation to be noticed.
    const DEADLINE: Duration = Duration::from_secs(5);

    /// A configuration reading `certificate` and `key`, reloading quickly.
    fn config_for(certificate: &Path, key: &Path) -> TlsConfig {
        TlsConfig {
            certificate_path: certificate.to_path_buf(),
            private_key_path: key.to_path_buf(),
            reload_debounce: HumanDuration::from_millis(100),
            ..TlsConfig::default()
        }
    }

    /// Wait until the watch serves `expected` as its leaf, or the deadline passes.
    fn leaf_becomes(watch: &TlsMaterialWatch, expected: &CertificateDer<'static>) -> bool {
        let started = Instant::now();
        while started.elapsed() < DEADLINE {
            if watch.current().leaf() == Some(expected) {
                return true;
            }
            thread::sleep(Duration::from_millis(25));
        }
        false
    }

    #[test]
    fn an_unreadable_initial_certificate_fails_start() -> Result<(), Box<dyn Error>> {
        let files = IdentityFiles::write(&TestAuthority::generate()?.issue_server_identity()?)?;
        fs::write(files.certificate_path(), b"not a certificate\n")?;
        let outcome = TlsMaterialWatch::start(&config_for(files.certificate_path(), files.private_key_path())).map(|_| ());
        assert!(
            matches!(outcome, Err(TlsWatchError::Material(TlsMaterialError::NoCertificates { .. }))),
            "{outcome:?}"
        );
        Ok(())
    }

    #[test]
    fn rewriting_the_pair_publishes_new_material() -> Result<(), Box<dyn Error>> {
        let authority = TestAuthority::generate()?;
        let first = authority.issue_server_identity()?;
        let second = authority.issue_server_identity()?;
        let files = IdentityFiles::write(&first)?;
        let watch = TlsMaterialWatch::start(&config_for(files.certificate_path(), files.private_key_path()))?;
        pretty_assertions::assert_eq!(watch.current().leaf(), Some(first.certificate()));
        files.replace(&second)?;
        assert!(leaf_becomes(&watch, second.certificate()), "the rotated certificate is published");
        Ok(())
    }

    #[test]
    fn an_invalid_rewrite_keeps_the_previous_material() -> Result<(), Box<dyn Error>> {
        let authority = TestAuthority::generate()?;
        let first = authority.issue_server_identity()?;
        let second = authority.issue_server_identity()?;
        let files = IdentityFiles::write(&first)?;
        let watch = TlsMaterialWatch::start(&config_for(files.certificate_path(), files.private_key_path()))?;
        fs::write(files.certificate_path(), b"garbage\n")?;
        // Past the debounce with a margin for the reload to have run.
        thread::sleep(Duration::from_millis(600));
        pretty_assertions::assert_eq!(watch.current().leaf(), Some(first.certificate()), "the broken rotation is ignored");
        files.replace(&second)?;
        assert!(leaf_becomes(&watch, second.certificate()), "a later valid rotation is still picked up");
        Ok(())
    }

    /// A Kubernetes Secret volume exposes each file as a symlink through a
    /// `..data` symlink, and rotates by renaming a new `..data` over the old one;
    /// the configured paths themselves are never written.
    #[cfg(unix)]
    #[test]
    fn a_symlinked_directory_swap_is_detected() -> Result<(), Box<dyn Error>> {
        let authority = TestAuthority::generate()?;
        let first = authority.issue_server_identity()?;
        let second = authority.issue_server_identity()?;
        let volume = TempDir::new()?;
        let write_generation = |name: &str, identity: &TestIdentity| -> Result<(), Box<dyn Error>> {
            let generation = volume.path().join(name);
            fs::create_dir(&generation)?;
            fs::write(generation.join("tls.crt"), identity.certificate_pem())?;
            fs::write(generation.join("tls.key"), identity.private_key_pem())?;
            Ok(())
        };
        write_generation("..2026_09_27_first", &first)?;
        symlink("..2026_09_27_first", volume.path().join("..data"))?;
        symlink("..data/tls.crt", volume.path().join("tls.crt"))?;
        symlink("..data/tls.key", volume.path().join("tls.key"))?;
        let watch = TlsMaterialWatch::start(&config_for(&volume.path().join("tls.crt"), &volume.path().join("tls.key")))?;
        pretty_assertions::assert_eq!(watch.current().leaf(), Some(first.certificate()));

        write_generation("..2026_09_27_second", &second)?;
        symlink("..2026_09_27_second", volume.path().join("..data_tmp"))?;
        fs::rename(volume.path().join("..data_tmp"), volume.path().join("..data"))?;
        assert!(leaf_becomes(&watch, second.certificate()), "the swapped generation is published");
        Ok(())
    }
}
