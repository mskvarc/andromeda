//! The scheme the listener a request arrived on speaks.

use crate::api_root_scheme::ApiRootScheme;
use derive_more::{Constructor, From, Into};

/// The scheme of the listener a request arrived on, carried as a request
/// extension.
///
/// Each listener installs its own: a TLS-terminating TCP listener and the
/// HTTP/3 listener speak `https`, a plaintext listener `http`. It is the
/// scheme of every URL the broker mints from the request origin unless a
/// trusted reverse proxy forwards another. A request carrying no extension (a
/// router assembled without a listener, as in a unit test) is treated as
/// plaintext, so it can never be talked into minting an `https` URL.
#[derive(Clone, Copy, Constructor, Debug, Eq, From, Into, PartialEq)]
pub struct ListenerScheme(ApiRootScheme);

#[cfg(test)]
mod tests {
    use crate::{api_root_scheme::ApiRootScheme, listener_scheme::ListenerScheme};

    #[test]
    fn listener_scheme_carries_its_api_root_scheme() {
        for scheme in [ApiRootScheme::Http, ApiRootScheme::Https] {
            pretty_assertions::assert_eq!(ApiRootScheme::from(ListenerScheme::from(scheme)), scheme);
        }
    }
}
