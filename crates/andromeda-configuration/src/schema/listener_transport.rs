//! What a TCP listener speaks on the wire.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// What a TCP listener speaks on the wire.
///
/// ETSI GS CIM 009 v1.9.1 clause 6.2 requires the API to support HTTP over TLS
/// and calls plaintext HTTP "not recommended". A listener either terminates
/// that TLS itself, from the `[tls]` identity, or serves plaintext behind a
/// reverse proxy that terminates it; it never does
/// both on one port.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Display, EnumString, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
#[schemars(description = "What a TCP listener speaks on the wire: plaintext HTTP or HTTP over TLS terminated in the broker.")]
pub enum ListenerTransport {
    /// Plaintext HTTP/1.1 and HTTP/2, for deployment behind a TLS terminator.
    #[default]
    #[schemars(
        description = "Serves plaintext HTTP/1.1 and HTTP/2, for a deployment behind a reverse proxy or load balancer that terminates TLS. NGSI-LD calls plaintext HTTP not recommended."
    )]
    Plaintext,
    /// HTTP/1.1 and HTTP/2 over TLS terminated in process with the `[tls]` identity.
    #[schemars(description = "Terminates TLS in the broker with the `[tls]` certificate and serves HTTP/1.1 and HTTP/2 over it.")]
    Tls,
}
