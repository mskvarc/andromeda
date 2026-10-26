//! The `[tls]` configuration section: where the broker's identity lives and
//! how its handshakes are negotiated.
//!
//! The section is inert on its own. It is read only when a listener is
//! configured to terminate TLS -- `server.transport = "tls"`, an enabled
//! `[http3]` listener, or a `separate-port` management listener with
//! `management.transport = "tls"` -- and the composition layer asks for its
//! [`TlsMaterialSources`] only then, so an unconfigured broker is never refused
//! for lacking a certificate it does not use.

use crate::{
    human_duration::HumanDuration,
    schema::tls_material_sources::{ClientTrust, TlsMaterialSources},
};
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, path::PathBuf};
use strum::{Display, EnumString};
use thiserror::Error;
use toml_comment::TomlComment;

/// Whether the broker asks a connecting client for a certificate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Display, EnumString)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum ClientAuthentication {
    /// No client certificate is requested; authentication, if any, is an
    /// application-layer concern.
    #[default]
    None,
    /// A client certificate is requested and verified when presented, but a
    /// client presenting none is still served. A certificate from an unknown
    /// issuer fails the handshake.
    Optional,
    /// Every client must present a certificate issued by the configured CA.
    Required,
}

/// The oldest TLS version a TCP handshake may negotiate.
///
/// Clause 6.2 requires TLS 1.2 support, so `tls1.2` is the default; `tls1.3`
/// is the stricter choice for a deployment whose clients all speak it. QUIC
/// carries TLS 1.3 only (RFC 9001) and ignores this setting.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Display, EnumString)]
pub enum MinimumProtocolVersion {
    /// TLS 1.2 and TLS 1.3.
    #[default]
    #[serde(rename = "tls1.2")]
    #[strum(serialize = "tls1.2")]
    Tls12,
    /// TLS 1.3 only.
    #[serde(rename = "tls1.3")]
    #[strum(serialize = "tls1.3")]
    Tls13,
}

/// A `[tls]` section that cannot supply a listener with an identity.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum TlsConfigError {
    /// No certificate chain path was configured.
    #[error("tls.certificate_path must be set when a listener terminates TLS")]
    MissingCertificate,
    /// No private key path was configured.
    #[error("tls.private_key_path must be set when a listener terminates TLS")]
    MissingPrivateKey,
    /// Client certificates are requested but nothing says which issuer to trust.
    #[error("tls.client_ca_path must be set when tls.client_authentication is \"optional\" or \"required\"")]
    MissingClientCa,
    /// A client CA bundle was configured while client certificates are not requested.
    #[error("tls.client_ca_path is set but tls.client_authentication is \"none\"; set client_authentication or remove the bundle")]
    ClientCaWithoutClientAuthentication,
}

/// The broker's TLS identity and handshake policy, shared by every listener
/// that terminates TLS.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TlsConfig {
    /// PEM-encoded certificate chain the broker presents, leaf first.
    #[toml_comment(inline)]
    pub certificate_path: PathBuf,
    /// PEM-encoded private key for the leaf certificate (PKCS#8, SEC1 or PKCS#1).
    #[toml_comment(inline)]
    pub private_key_path: PathBuf,
    /// Whether clients are asked for a certificate: `none`, `optional` or `required`.
    #[toml_comment(inline)]
    pub client_authentication: ClientAuthentication,
    /// PEM bundle of the CAs client certificates must chain to. Required
    /// exactly when `client_authentication` is not `none`.
    #[toml_comment(inline)]
    pub client_ca_path: PathBuf,
    /// Oldest TLS version a TCP handshake may negotiate: `tls1.2` or `tls1.3`.
    #[toml_comment(inline)]
    pub minimum_protocol_version: MinimumProtocolVersion,
    /// How long a TCP client may take to complete its handshake before the
    /// connection is dropped, in human-readable form (`"10s"`).
    #[toml_comment(inline)]
    pub handshake_timeout: HumanDuration,
    /// How long the certificate files must stay quiet after a change before
    /// they are reloaded, in human-readable form (`"2s"`).
    #[toml_comment(inline)]
    pub reload_debounce: HumanDuration,
}

impl Default for TlsConfig {
    // Manual rather than derived: the two timeouts are concrete durations a
    // field-wise derive cannot express.
    fn default() -> TlsConfig {
        TlsConfig {
            certificate_path: PathBuf::new(),
            private_key_path: PathBuf::new(),
            client_authentication: ClientAuthentication::None,
            client_ca_path: PathBuf::new(),
            minimum_protocol_version: MinimumProtocolVersion::Tls12,
            handshake_timeout: HumanDuration::from_secs(10),
            reload_debounce: HumanDuration::from_secs(2),
        }
    }
}

impl TlsConfig {
    /// Validate the section as the identity of a TLS-terminating listener and
    /// return the files it is loaded from.
    ///
    /// # Errors
    ///
    /// Returns [`TlsConfigError::MissingCertificate`] or
    /// [`TlsConfigError::MissingPrivateKey`] when a key-pair path is empty,
    /// [`TlsConfigError::MissingClientCa`] when client certificates are
    /// requested without a CA bundle, and
    /// [`TlsConfigError::ClientCaWithoutClientAuthentication`] when a bundle is
    /// configured that nothing would read.
    pub fn material_sources(&self) -> Result<TlsMaterialSources<'_>, TlsConfigError> {
        if self.certificate_path.as_os_str().is_empty() {
            return Err(TlsConfigError::MissingCertificate);
        }
        if self.private_key_path.as_os_str().is_empty() {
            return Err(TlsConfigError::MissingPrivateKey);
        }
        let bundle = (!self.client_ca_path.as_os_str().is_empty()).then_some(Cow::Borrowed(self.client_ca_path.as_path()));
        let client_trust = match (self.client_authentication, bundle) {
            (ClientAuthentication::None, None) => ClientTrust::Anonymous,
            (ClientAuthentication::None, Some(_)) => return Err(TlsConfigError::ClientCaWithoutClientAuthentication),
            (ClientAuthentication::Optional | ClientAuthentication::Required, None) => return Err(TlsConfigError::MissingClientCa),
            (ClientAuthentication::Optional, Some(bundle)) => ClientTrust::Optional(bundle),
            (ClientAuthentication::Required, Some(bundle)) => ClientTrust::Required(bundle),
        };
        Ok(TlsMaterialSources::new(
            Cow::Borrowed(&self.certificate_path),
            Cow::Borrowed(&self.private_key_path),
            client_trust,
        ))
    }
}

#[cfg(test)]
mod tests {
    use crate::schema::{
        tls::{ClientAuthentication, MinimumProtocolVersion, TlsConfig, TlsConfigError},
        tls_material_sources::ClientTrust,
    };
    use pretty_assertions::assert_eq;
    use std::{
        error::Error,
        path::{Path, PathBuf},
        str::FromStr,
    };

    /// A configuration naming a key pair, with client authentication as given.
    fn with_key_pair(client_authentication: ClientAuthentication, client_ca_path: &str) -> TlsConfig {
        TlsConfig {
            certificate_path: PathBuf::from("/etc/andromeda/broker.crt"),
            private_key_path: PathBuf::from("/etc/andromeda/broker.key"),
            client_authentication,
            client_ca_path: PathBuf::from(client_ca_path),
            ..TlsConfig::default()
        }
    }

    #[test]
    fn default_config_yields_no_material_sources_error_only_when_asked() {
        let config = TlsConfig::default();
        assert_eq!(config.client_authentication, ClientAuthentication::None);
        assert_eq!(config.minimum_protocol_version, MinimumProtocolVersion::Tls12);
        assert_eq!(config.material_sources().map(|_| ()), Err(TlsConfigError::MissingCertificate));
    }

    #[test]
    fn material_sources_without_private_key_are_rejected() {
        let config = TlsConfig {
            certificate_path: PathBuf::from("/etc/andromeda/broker.crt"),
            ..TlsConfig::default()
        };
        assert_eq!(config.material_sources().map(|_| ()), Err(TlsConfigError::MissingPrivateKey));
    }

    #[test]
    fn required_client_authentication_without_ca_is_rejected() {
        let config = with_key_pair(ClientAuthentication::Required, "");
        assert_eq!(config.material_sources().map(|_| ()), Err(TlsConfigError::MissingClientCa));
    }

    #[test]
    fn optional_client_authentication_without_ca_is_rejected() {
        let config = with_key_pair(ClientAuthentication::Optional, "");
        assert_eq!(config.material_sources().map(|_| ()), Err(TlsConfigError::MissingClientCa));
    }

    #[test]
    fn client_ca_without_client_authentication_is_rejected() {
        let config = with_key_pair(ClientAuthentication::None, "/etc/andromeda/clients.pem");
        assert_eq!(config.material_sources().map(|_| ()), Err(TlsConfigError::ClientCaWithoutClientAuthentication));
    }

    #[test]
    fn complete_sources_expose_their_paths() -> Result<(), TlsConfigError> {
        let config = with_key_pair(ClientAuthentication::Required, "/etc/andromeda/clients.pem");
        let sources = config.material_sources()?;
        assert_eq!(sources.certificate_path(), config.certificate_path);
        assert_eq!(sources.private_key_path(), config.private_key_path);
        assert_eq!(sources.client_trust(), &ClientTrust::Required(Path::new("/etc/andromeda/clients.pem").into()));
        let optional = with_key_pair(ClientAuthentication::Optional, "/etc/andromeda/clients.pem");
        assert_eq!(
            optional.material_sources()?.client_trust().bundle_path(),
            Some(Path::new("/etc/andromeda/clients.pem"))
        );
        let anonymous = with_key_pair(ClientAuthentication::None, "");
        assert_eq!(anonymous.material_sources()?.client_trust(), &ClientTrust::Anonymous);
        Ok(())
    }

    #[test]
    fn client_authentication_round_trips_kebab_case() -> Result<(), Box<dyn Error>> {
        for (mode, token) in [
            (ClientAuthentication::None, "none"),
            (ClientAuthentication::Optional, "optional"),
            (ClientAuthentication::Required, "required"),
        ] {
            assert_eq!(serde_json::to_string(&mode)?, format!("\"{token}\""));
            assert_eq!(serde_json::from_str::<ClientAuthentication>(&format!("\"{token}\""))?, mode);
            assert_eq!(ClientAuthentication::from_str(token)?, mode);
            assert_eq!(mode.to_string(), token);
        }
        Ok(())
    }

    #[test]
    fn minimum_protocol_version_round_trips() -> Result<(), Box<dyn Error>> {
        for (version, token) in [(MinimumProtocolVersion::Tls12, "tls1.2"), (MinimumProtocolVersion::Tls13, "tls1.3")] {
            assert_eq!(serde_json::to_string(&version)?, format!("\"{token}\""));
            assert_eq!(serde_json::from_str::<MinimumProtocolVersion>(&format!("\"{token}\""))?, version);
            assert_eq!(MinimumProtocolVersion::from_str(token)?, version);
            assert_eq!(version.to_string(), token);
        }
        assert!(serde_json::from_str::<MinimumProtocolVersion>("\"tls12\"").is_err());
        assert!(MinimumProtocolVersion::from_str("tls12").is_err());
        Ok(())
    }

    #[test]
    fn tls_config_round_trips_through_toml() -> Result<(), Box<dyn Error>> {
        let config = TlsConfig {
            minimum_protocol_version: MinimumProtocolVersion::Tls13,
            ..with_key_pair(ClientAuthentication::Optional, "/etc/andromeda/clients.pem")
        };
        let rendered = toml::to_string(&config)?;
        assert!(rendered.contains("minimum_protocol_version = \"tls1.3\""), "{rendered}");
        assert!(rendered.contains("client_authentication = \"optional\""), "{rendered}");
        assert_eq!(toml::from_str::<TlsConfig>(&rendered)?, config);
        Ok(())
    }
}
