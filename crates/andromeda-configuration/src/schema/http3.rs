//! Opt-in HTTP/3 listener configuration.
//!
//! HTTP/3 runs over QUIC (RFC 9114), which has no cleartext mode: a QUIC
//! endpoint always terminates TLS 1.3 in process. The listener therefore
//! presents the broker's shared `[tls]` identity, the same one a TLS-terminating
//! TCP listener presents on the same host and port. It is disabled by default;
//! enabling it without that identity is a configuration error the composition
//! layer reports rather than a silent no-op.

use crate::human_duration::HumanDuration;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};
use toml_comment::TomlComment;

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

/// Address-independent HTTP/3 listener policy.
///
/// The listener has no address of its own: it binds the same host and port as
/// the TCP listener, differing only in transport (UDP/QUIC rather than TCP).
/// Its certificate is the `[tls]` identity.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Http3Config {
    /// Whether the QUIC/HTTP/3 listener is served alongside the TCP listener.
    /// Enabling it requires the `[tls]` certificate and private key.
    #[toml_comment(inline)]
    pub mode: Http3Mode,
    /// How long a client may cache the `Alt-Svc` advertisement of this listener
    /// before rediscovering it (RFC 7838 `ma`), in human-readable form
    /// (`"24h"`).
    #[toml_comment(inline)]
    pub advertisement_lifetime: HumanDuration,
}

impl Default for Http3Config {
    // Manual rather than derived: the one-day advertisement lifetime is a
    // concrete duration a field-wise derive cannot express.
    fn default() -> Http3Config {
        Http3Config {
            mode: Http3Mode::Disabled,
            advertisement_lifetime: HumanDuration::from_secs(86_400),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::schema::http3::{Http3Config, Http3Mode};
    use pretty_assertions::assert_eq;
    use std::error::Error;

    #[test]
    fn the_listener_is_disabled_by_default() {
        assert_eq!(Http3Config::default().mode, Http3Mode::Disabled);
    }

    #[test]
    fn the_mode_round_trips_through_toml() -> Result<(), Box<dyn Error>> {
        let config = Http3Config {
            mode: Http3Mode::Enabled,
            ..Http3Config::default()
        };
        let rendered = toml::to_string(&config)?;
        let parsed: Http3Config = toml::from_str(&rendered)?;
        assert_eq!(parsed.mode, Http3Mode::Enabled);
        assert_eq!(parsed.advertisement_lifetime, config.advertisement_lifetime);
        Ok(())
    }
}
