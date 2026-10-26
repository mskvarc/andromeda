//! What a TCP listener speaks on the wire.

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// What a TCP listener speaks on the wire.
///
/// ETSI GS CIM 009 v1.9.1 clause 6.2 requires the API to support HTTP over TLS
/// and calls plaintext HTTP "not recommended". A listener either terminates
/// that TLS itself, from the `[tls]` identity, or serves plaintext behind a
/// reverse proxy that terminates it; it never does
/// both on one port.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Display, EnumString)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum ListenerTransport {
    /// Plaintext HTTP/1.1 and HTTP/2, for deployment behind a TLS terminator.
    #[default]
    Plaintext,
    /// HTTP/1.1 and HTTP/2 over TLS terminated in process with the `[tls]` identity.
    Tls,
}
