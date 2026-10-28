//! The `[tls]` table: where the broker's identity lives and how its handshakes
//! are negotiated.
//!
//! The table is inert on its own. It is read only when a listener terminates
//! TLS (`server.transport = "tls"`, an enabled `[server.http3]` listener, or a
//! `separate-port` management listener with `management.transport = "tls"`),
//! and the composition layer asks for its [`TlsMaterialSources`] only then, so
//! an unconfigured broker is never refused for lacking a certificate it does
//! not use.

use crate::{
    human_duration::HumanDuration,
    schema::tls_material_sources::{ClientTrust, TlsMaterialSources},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, path::PathBuf};
use strum::{Display, EnumString};
use thiserror::Error;

/// Whether the broker asks a connecting client for a certificate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Display, EnumString, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
#[schemars(description = "Whether the broker asks a connecting client for a certificate (mutual TLS).")]
pub enum ClientAuthenticationMode {
    /// No client certificate is requested; authentication, if any, is an
    /// application-layer concern.
    #[default]
    #[schemars(description = "Asks for no client certificate. Authentication, if any, happens above TLS.")]
    Disabled,
    /// A client certificate is requested and verified when presented, but a
    /// client presenting none is still served. A certificate from an unknown
    /// issuer fails the handshake.
    #[schemars(
        description = "Verifies a client certificate when the client sends one and still serves a client without. A certificate from an issuer outside `tls.client_authentication.ca` fails the handshake."
    )]
    Optional,
    /// Every client must present a certificate issued by the configured CA.
    #[schemars(description = "Serves only clients that present a certificate chaining to `tls.client_authentication.ca`.")]
    Required,
}

/// The oldest TLS version a TCP handshake may negotiate.
///
/// Clause 6.2 requires TLS 1.2 support, so `tls1.2` is the default; `tls1.3`
/// is the stricter choice for a deployment whose clients all speak it. QUIC
/// carries TLS 1.3 only (RFC 9001) and ignores this setting.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Display, EnumString, JsonSchema)]
#[schemars(description = "The oldest TLS version a TCP handshake may negotiate. HTTP/3 always uses TLS 1.3 and ignores it.")]
pub enum MinimumProtocolVersion {
    /// TLS 1.2 and TLS 1.3.
    #[default]
    #[serde(rename = "tls1.2")]
    #[strum(serialize = "tls1.2")]
    #[schemars(description = "Accepts TLS 1.2 and TLS 1.3. NGSI-LD requires TLS 1.2 support.")]
    Tls12,
    /// TLS 1.3 only.
    #[serde(rename = "tls1.3")]
    #[strum(serialize = "tls1.3")]
    #[schemars(description = "Accepts TLS 1.3 only, for a deployment whose clients all speak it.")]
    Tls13,
}

/// A `[tls]` table that cannot supply a listener with an identity.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum TlsConfigError {
    /// No certificate chain was configured.
    #[error("tls.certificate must be set when a listener terminates TLS")]
    MissingCertificate,
    /// No private key was configured.
    #[error("tls.private_key must be set when a listener terminates TLS")]
    MissingPrivateKey,
    /// Client certificates are requested but nothing says which issuer to trust.
    #[error("tls.client_authentication.ca must be set when tls.client_authentication.mode is \"optional\" or \"required\"")]
    MissingClientCa,
    /// A client CA bundle was configured while client certificates are not requested.
    #[error("tls.client_authentication.ca is set but tls.client_authentication.mode is \"disabled\"; set the mode or remove the bundle")]
    ClientCaWithoutClientAuthentication,
}

/// Whether connecting clients are asked for a certificate, and which issuer
/// it must chain to.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[schemars(description = "Whether clients must present a certificate, and which issuers it must chain to.")]
pub struct ClientAuthenticationConfig {
    /// Whether clients are asked for a certificate: `disabled`, `optional` or
    /// `required`.
    #[schemars(description = "Whether clients are asked for a certificate, and whether one is required.")]
    pub mode: ClientAuthenticationMode,
    /// PEM bundle of the CAs client certificates must chain to. Required
    /// exactly when `mode` is not `disabled`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        title = "path",
        description = "The PEM bundle of the CAs client certificates must chain to. Left out, no client certificate can be verified."
    )]
    #[schemars(extend("x-andromeda-apply" = "reload"))]
    pub ca: Option<PathBuf>,
}

/// The broker's TLS identity and handshake policy, shared by every listener
/// that terminates TLS.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[schemars(
    description = "The TLS identity and handshake policy every TLS-terminating listener shares: the API listener with `server.transport` at `\"tls\"`, the HTTP/3 listener, and a separate management listener with `management.transport` at `\"tls\"`. Nothing reads it while no listener terminates TLS."
)]
pub struct TlsConfig {
    /// PEM-encoded certificate chain the broker presents, leaf first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        title = "path",
        description = "The PEM certificate chain the broker presents, leaf first. Left out, no listener can terminate TLS. The file is reloaded when it changes."
    )]
    #[schemars(extend("x-andromeda-apply" = "reload"))]
    pub certificate: Option<PathBuf>,
    /// PEM-encoded private key for the leaf certificate (PKCS#8, SEC1 or PKCS#1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        title = "path",
        description = "The PEM private key of the leaf certificate, as PKCS#8, SEC1 or PKCS#1. Left out, no listener can terminate TLS. The file is reloaded when it changes."
    )]
    #[schemars(extend("x-andromeda-apply" = "reload"))]
    pub private_key: Option<PathBuf>,
    /// Oldest TLS version a TCP handshake may negotiate: `tls1.2` or `tls1.3`.
    #[schemars(description = "The oldest TLS version a TCP connection may negotiate. HTTP/3 always uses TLS 1.3.")]
    pub minimum_protocol_version: MinimumProtocolVersion,
    /// How long a TCP client may take to complete its handshake before the
    /// connection is dropped, in human-readable form (`"10s"`).
    #[schemars(description = "How long a TCP client may take to finish its TLS handshake before the connection is dropped.")]
    pub handshake_timeout: HumanDuration,
    /// How long the certificate files must stay unchanged after a change
    /// before they are reloaded, in human-readable form (`"2s"`), so a
    /// certificate and key replaced one after the other load together.
    #[schemars(
        description = "How long the certificate files must stay unchanged after an edit before the broker loads them again, so a certificate and key replaced one after the other load together."
    )]
    pub reload_settle_after: HumanDuration,
    /// Client certificate authentication.
    #[schemars(description = "Client certificate authentication (mutual TLS).")]
    pub client_authentication: ClientAuthenticationConfig,
}

impl Default for TlsConfig {
    // Manual rather than derived: the two timeouts are concrete durations a
    // field-wise derive cannot express.
    fn default() -> TlsConfig {
        TlsConfig {
            certificate: None,
            private_key: None,
            minimum_protocol_version: MinimumProtocolVersion::Tls12,
            handshake_timeout: HumanDuration::from_secs(10),
            reload_settle_after: HumanDuration::from_secs(2),
            client_authentication: ClientAuthenticationConfig::default(),
        }
    }
}

impl TlsConfig {
    /// Validate the table as the identity of a TLS-terminating listener and
    /// return the files it is loaded from.
    ///
    /// # Errors
    ///
    /// Returns [`TlsConfigError::MissingCertificate`] or
    /// [`TlsConfigError::MissingPrivateKey`] when a key-pair file is not
    /// configured, [`TlsConfigError::MissingClientCa`] when client
    /// certificates are requested without a CA bundle, and
    /// [`TlsConfigError::ClientCaWithoutClientAuthentication`] when a bundle
    /// is configured that nothing would read.
    pub fn material_sources(&self) -> Result<TlsMaterialSources<'_>, TlsConfigError> {
        let certificate = self.certificate.as_deref().ok_or(TlsConfigError::MissingCertificate)?;
        let private_key = self.private_key.as_deref().ok_or(TlsConfigError::MissingPrivateKey)?;
        let bundle = self.client_authentication.ca.as_deref().map(Cow::Borrowed);
        let client_trust = match (self.client_authentication.mode, bundle) {
            (ClientAuthenticationMode::Disabled, None) => ClientTrust::Anonymous,
            (ClientAuthenticationMode::Disabled, Some(_)) => return Err(TlsConfigError::ClientCaWithoutClientAuthentication),
            (ClientAuthenticationMode::Optional | ClientAuthenticationMode::Required, None) => return Err(TlsConfigError::MissingClientCa),
            (ClientAuthenticationMode::Optional, Some(bundle)) => ClientTrust::Optional(bundle),
            (ClientAuthenticationMode::Required, Some(bundle)) => ClientTrust::Required(bundle),
        };
        Ok(TlsMaterialSources::new(Cow::Borrowed(certificate), Cow::Borrowed(private_key), client_trust))
    }
}

#[cfg(test)]
mod tests {
    use crate::schema::{
        tls::{ClientAuthenticationConfig, ClientAuthenticationMode, MinimumProtocolVersion, TlsConfig, TlsConfigError},
        tls_material_sources::ClientTrust,
    };
    use pretty_assertions::assert_eq;
    use std::{
        error::Error,
        path::{Path, PathBuf},
        str::FromStr,
    };

    /// A configuration naming a key pair, with client authentication as given.
    fn with_key_pair(mode: ClientAuthenticationMode, ca: Option<&str>) -> TlsConfig {
        TlsConfig {
            certificate: Some(PathBuf::from("/etc/andromeda/broker.crt")),
            private_key: Some(PathBuf::from("/etc/andromeda/broker.key")),
            client_authentication: ClientAuthenticationConfig {
                mode,
                ca: ca.map(PathBuf::from),
            },
            ..TlsConfig::default()
        }
    }

    #[test]
    fn default_config_yields_no_material_sources_error_only_when_asked() {
        let config = TlsConfig::default();
        assert_eq!(config.client_authentication.mode, ClientAuthenticationMode::Disabled);
        assert_eq!(config.minimum_protocol_version, MinimumProtocolVersion::Tls12);
        assert_eq!(config.material_sources().map(|_| ()), Err(TlsConfigError::MissingCertificate));
    }

    #[test]
    fn material_sources_without_private_key_are_rejected() {
        let config = TlsConfig {
            certificate: Some(PathBuf::from("/etc/andromeda/broker.crt")),
            ..TlsConfig::default()
        };
        assert_eq!(config.material_sources().map(|_| ()), Err(TlsConfigError::MissingPrivateKey));
    }

    #[test]
    fn requested_client_certificates_without_ca_are_rejected() {
        for mode in [ClientAuthenticationMode::Optional, ClientAuthenticationMode::Required] {
            assert_eq!(with_key_pair(mode, None).material_sources().map(|_| ()), Err(TlsConfigError::MissingClientCa));
        }
    }

    #[test]
    fn client_ca_without_client_authentication_is_rejected() {
        let config = with_key_pair(ClientAuthenticationMode::Disabled, Some("/etc/andromeda/clients.pem"));
        assert_eq!(config.material_sources().map(|_| ()), Err(TlsConfigError::ClientCaWithoutClientAuthentication));
    }

    #[test]
    fn complete_sources_expose_their_paths() -> Result<(), TlsConfigError> {
        let config = with_key_pair(ClientAuthenticationMode::Required, Some("/etc/andromeda/clients.pem"));
        let sources = config.material_sources()?;
        assert_eq!(sources.certificate_path(), Path::new("/etc/andromeda/broker.crt"));
        assert_eq!(sources.private_key_path(), Path::new("/etc/andromeda/broker.key"));
        assert_eq!(sources.client_trust(), &ClientTrust::Required(Path::new("/etc/andromeda/clients.pem").into()));
        let optional = with_key_pair(ClientAuthenticationMode::Optional, Some("/etc/andromeda/clients.pem"));
        assert_eq!(
            optional.material_sources()?.client_trust().bundle_path(),
            Some(Path::new("/etc/andromeda/clients.pem"))
        );
        let anonymous = with_key_pair(ClientAuthenticationMode::Disabled, None);
        assert_eq!(anonymous.material_sources()?.client_trust(), &ClientTrust::Anonymous);
        Ok(())
    }

    #[test]
    fn client_authentication_mode_round_trips_kebab_case() -> Result<(), Box<dyn Error>> {
        for (mode, token) in [
            (ClientAuthenticationMode::Disabled, "disabled"),
            (ClientAuthenticationMode::Optional, "optional"),
            (ClientAuthenticationMode::Required, "required"),
        ] {
            assert_eq!(serde_json::to_string(&mode)?, format!("\"{token}\""));
            assert_eq!(serde_json::from_str::<ClientAuthenticationMode>(&format!("\"{token}\""))?, mode);
            assert_eq!(ClientAuthenticationMode::from_str(token)?, mode);
            assert_eq!(mode.to_string(), token);
        }
        assert!(serde_json::from_str::<ClientAuthenticationMode>("\"none\"").is_err());
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
            ..with_key_pair(ClientAuthenticationMode::Optional, Some("/etc/andromeda/clients.pem"))
        };
        let rendered = toml::to_string(&config)?;
        assert!(rendered.contains("minimum_protocol_version = \"tls1.3\""), "{rendered}");
        assert!(rendered.contains("[client_authentication]\nmode = \"optional\""), "{rendered}");
        assert_eq!(toml::from_str::<TlsConfig>(&rendered)?, config);
        Ok(())
    }

    #[test]
    fn absent_files_are_left_out_rather_than_written_empty() -> Result<(), Box<dyn Error>> {
        let rendered = toml::to_string(&TlsConfig::default())?;
        assert!(
            !rendered.contains("certificate") && !rendered.contains("private_key") && !rendered.contains("ca ="),
            "{rendered}"
        );
        Ok(())
    }
}
