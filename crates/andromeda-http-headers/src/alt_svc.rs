//! The `Alt-Svc` advertisement of an alternative HTTP/3 endpoint.
//!
//! RFC 7838 defines `Alt-Svc` as the way an origin tells a client that the same
//! authority is reachable over another protocol on a given host and port. The
//! broker uses it on its TCP (HTTP/1.1 and HTTP/2) responses to advertise the
//! HTTP/3 endpoint that shares the listener's port, so a client that first
//! arrives over TCP can upgrade to QUIC on a later request. Advertising `h3`
//! (RFC 9114 clause 3.3) is only honest once the QUIC listener is actually
//! bound, so the composition layer builds this value only when HTTP/3 is served.

use http::{HeaderName, HeaderValue, header::InvalidHeaderValue};
use std::{num::NonZeroU16, time::Duration};
use thiserror::Error;

/// The lowercase `Alt-Svc` field name this advertisement is transmitted under.
static ALT_SVC: HeaderName = HeaderName::from_static("alt-svc");

/// An `Alt-Svc` value could not be rendered into a header value.
#[derive(Debug, Error)]
#[error("alt-svc advertisement is not a valid header value: {0}")]
pub struct InvalidAdvertisement(#[from] InvalidHeaderValue);

/// An `Alt-Svc` value advertising HTTP/3 over QUIC on a port.
///
/// The rendered value has the RFC 7838 form `h3=":<port>"; ma=<seconds>`, where
/// the empty host means "the same host as this origin" and `ma` is how long the
/// client may cache the advertisement before rediscovering it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AltSvcAdvertisement(HeaderValue);

impl AltSvcAdvertisement {
    /// Advertise HTTP/3 on `port`, cached for `max_age`.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidAdvertisement`] when the rendered value is not a valid
    /// header value. The `h3=":<port>"; ma=<seconds>` grammar is fixed visible
    /// ASCII, so this does not occur for any `port` and `max_age`; the fallible
    /// signature avoids a panicking construction on the startup path.
    pub fn h3(port: NonZeroU16, max_age: Duration) -> Result<AltSvcAdvertisement, InvalidAdvertisement> {
        let rendered = format!("h3=\":{port}\"; ma={}", max_age.as_secs());
        Ok(AltSvcAdvertisement(HeaderValue::from_str(&rendered)?))
    }

    /// The `Alt-Svc` field name.
    #[must_use]
    pub fn name() -> &'static HeaderName {
        &ALT_SVC
    }

    /// The rendered header value.
    #[must_use]
    pub const fn value(&self) -> &HeaderValue {
        &self.0
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::alt_svc::AltSvcAdvertisement;
    use std::{error::Error, num::NonZeroU16, time::Duration};

    #[test]
    fn an_advertisement_renders_the_rfc7838_h3_form() -> Result<(), Box<dyn Error>> {
        let port = NonZeroU16::new(443).expect("443 is non-zero");
        let advertisement = AltSvcAdvertisement::h3(port, Duration::from_hours(24))?;
        pretty_assertions::assert_eq!(advertisement.value().to_str()?, "h3=\":443\"; ma=86400");
        Ok(())
    }

    #[test]
    fn the_advertised_port_and_max_age_appear_in_the_value() -> Result<(), Box<dyn Error>> {
        let port = NonZeroU16::new(1056).expect("1056 is non-zero");
        let advertisement = AltSvcAdvertisement::h3(port, Duration::from_hours(1))?;
        pretty_assertions::assert_eq!(advertisement.value().to_str()?, "h3=\":1056\"; ma=3600");
        Ok(())
    }

    #[test]
    fn the_field_name_is_alt_svc() {
        pretty_assertions::assert_eq!(AltSvcAdvertisement::name().as_str(), "alt-svc");
    }
}
