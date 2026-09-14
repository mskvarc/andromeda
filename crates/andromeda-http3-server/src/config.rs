//! Opt-in HTTP/3 listener configuration.
//!
//! HTTP/3 runs over QUIC (RFC 9114), which has no cleartext mode: a QUIC
//! endpoint always terminates TLS 1.3 in process. The broker therefore needs a
//! certificate and private key of its own before it can advertise or serve
//! HTTP/3, unlike the plaintext TCP listener a reverse proxy fronts. The
//! listener is disabled by default; enabling it without a key pair is a
//! configuration error rather than a silent no-op.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use strum::{Display, EnumString};
use thiserror::Error;
use toml_comment::TomlComment;

/// Invalid HTTP/3 listener configuration.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum Http3ConfigError {
    /// HTTP/3 was enabled without a certificate path.
    #[error("http3.certificate_path must be set when http3.mode is enabled")]
    MissingCertificate,
    /// HTTP/3 was enabled without a private key path.
    #[error("http3.private_key_path must be set when http3.mode is enabled")]
    MissingPrivateKey,
}

/// Whether the QUIC/HTTP/3 listener is served.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum Http3Mode {
    /// Do not bind a QUIC endpoint and do not advertise HTTP/3.
    #[default]
    Disabled,
    /// Bind a QUIC endpoint sharing the TCP listener's port and advertise it.
    Enabled,
}

/// The certificate and key an active HTTP/3 listener terminates TLS with.
///
/// Borrowed from a validated [`Http3Config`]; its existence is the proof that
/// the listener is enabled and its key pair is present, so a caller holding one
/// never has to re-check the mode.
#[derive(Debug, Clone, Copy)]
pub struct Http3Activation<'config> {
    /// PEM-encoded certificate chain, leaf first.
    certificate_path: &'config Path,
    /// PEM-encoded PKCS#8 private key for the leaf certificate.
    private_key_path: &'config Path,
}

impl<'config> Http3Activation<'config> {
    /// The PEM certificate chain path.
    #[must_use]
    pub const fn certificate_path(&self) -> &'config Path {
        self.certificate_path
    }

    /// The PEM private key path.
    #[must_use]
    pub const fn private_key_path(&self) -> &'config Path {
        self.private_key_path
    }
}

/// Address-independent HTTP/3 listener policy.
///
/// The listener has no address of its own: it binds the same host and port as
/// the TCP listener, differing only in transport (UDP/QUIC rather than TCP).
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone)]
pub struct Http3Config {
    /// Whether the QUIC/HTTP/3 listener is served alongside the TCP listener.
    #[toml_comment(inline)]
    pub mode: Http3Mode,
    /// PEM-encoded certificate chain the QUIC endpoint presents, leaf first.
    #[toml_comment(inline)]
    pub certificate_path: PathBuf,
    /// PEM-encoded PKCS#8 private key for the leaf certificate.
    #[toml_comment(inline)]
    pub private_key_path: PathBuf,
    /// How long, in seconds, a client may cache the `Alt-Svc` advertisement of
    /// this listener before rediscovering it (RFC 7838 `ma`).
    pub advertisement_max_age_secs: u64,
}

impl Default for Http3Config {
    fn default() -> Http3Config {
        Http3Config {
            mode: Http3Mode::Disabled,
            certificate_path: PathBuf::new(),
            private_key_path: PathBuf::new(),
            advertisement_max_age_secs: 86_400,
        }
    }
}

impl Http3Config {
    /// Validate the listener configuration and return its activation when enabled.
    ///
    /// A disabled listener is always valid and yields [`None`]. An enabled
    /// listener yields [`Some`] activation only when both key-pair paths are
    /// present.
    ///
    /// # Errors
    ///
    /// Returns [`Http3ConfigError::MissingCertificate`] or
    /// [`Http3ConfigError::MissingPrivateKey`] when the listener is enabled but
    /// the corresponding path is empty.
    pub fn activation(&self) -> Result<Option<Http3Activation<'_>>, Http3ConfigError> {
        match self.mode {
            Http3Mode::Disabled => Ok(None),
            Http3Mode::Enabled => {
                if self.certificate_path.as_os_str().is_empty() {
                    return Err(Http3ConfigError::MissingCertificate);
                }
                if self.private_key_path.as_os_str().is_empty() {
                    return Err(Http3ConfigError::MissingPrivateKey);
                }
                Ok(Some(Http3Activation {
                    certificate_path: &self.certificate_path,
                    private_key_path: &self.private_key_path,
                }))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::{Http3Config, Http3ConfigError, Http3Mode};
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    #[test]
    fn a_disabled_listener_yields_no_activation() {
        let config = Http3Config::default();
        assert_eq!(config.mode, Http3Mode::Disabled);
        assert!(config.activation().expect("disabled is valid").is_none());
    }

    #[test]
    fn an_enabled_listener_without_a_certificate_is_rejected() {
        let config = Http3Config {
            mode: Http3Mode::Enabled,
            private_key_path: PathBuf::from("/etc/andromeda/broker.key"),
            ..Http3Config::default()
        };
        assert_eq!(config.activation().map(|_| ()), Err(Http3ConfigError::MissingCertificate));
    }

    #[test]
    fn an_enabled_listener_without_a_private_key_is_rejected() {
        let config = Http3Config {
            mode: Http3Mode::Enabled,
            certificate_path: PathBuf::from("/etc/andromeda/broker.crt"),
            ..Http3Config::default()
        };
        assert_eq!(config.activation().map(|_| ()), Err(Http3ConfigError::MissingPrivateKey));
    }

    #[test]
    fn an_enabled_listener_with_both_paths_yields_an_activation() {
        let config = Http3Config {
            mode: Http3Mode::Enabled,
            certificate_path: PathBuf::from("/etc/andromeda/broker.crt"),
            private_key_path: PathBuf::from("/etc/andromeda/broker.key"),
            ..Http3Config::default()
        };
        let activation = config.activation().expect("both paths present").expect("enabled yields activation");
        assert_eq!(activation.certificate_path(), config.certificate_path);
        assert_eq!(activation.private_key_path(), config.private_key_path);
    }

    #[test]
    fn the_mode_round_trips_through_toml() {
        let config = Http3Config {
            mode: Http3Mode::Enabled,
            certificate_path: PathBuf::from("/etc/andromeda/broker.crt"),
            private_key_path: PathBuf::from("/etc/andromeda/broker.key"),
            ..Http3Config::default()
        };
        let rendered = toml::to_string(&config).expect("config serialises to TOML");
        let parsed: Http3Config = toml::from_str(&rendered).expect("config parses from TOML");
        assert_eq!(parsed.mode, Http3Mode::Enabled);
    }
}
