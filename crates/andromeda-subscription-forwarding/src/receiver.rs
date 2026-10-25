//! The notification endpoint this broker advertises to context sources.

use andromeda_ngsi_ld_lexicon::dereferenceable_uri::DereferenceableUri;
use andromeda_ngsi_ld_model::subscription::Endpoint;
use mediatype::{MediaTypeBuf, names};
use url::Url;

/// The path of the inbound notification resource below the API base.
const NOTIFICATIONS_PATH: [&str; 3] = ["ngsi-ld", "v1", "notifications"];

/// The URL a context source notifies this broker at (clauses 5.8.1.4 and 5.8.6).
///
/// Derived once from the advertised origin: the URL has to be reachable from
/// the peer, so it cannot be the bind address this broker listens on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiverEndpoint(Url);

impl ReceiverEndpoint {
    /// The receiver below the advertised origin.
    #[must_use]
    pub fn from_advertised(advertised: &Url) -> Option<ReceiverEndpoint> {
        let mut url = advertised.clone(); // clone: the endpoint owns a URL distinct from the configured origin
        {
            let mut segments = url.path_segments_mut().ok()?;
            segments.pop_if_empty();
            // An origin already spelled with the API base is not doubled.
            let existing: Vec<String> = advertised
                .path_segments()
                .map(|segments| segments.filter(|s| !s.is_empty()).map(str::to_string).collect())
                .unwrap_or_default();
            let has_base = existing.len() >= 2 && existing[existing.len() - 2..] == NOTIFICATIONS_PATH[..2];
            let start = if has_base { 2 } else { 0 };
            segments.extend(&NOTIFICATIONS_PATH[start..]);
        }
        url.set_query(None);
        url.set_fragment(None);
        Some(ReceiverEndpoint(url))
    }

    /// The endpoint URL.
    #[must_use]
    pub const fn url(&self) -> &Url {
        &self.0
    }

    /// The notification endpoint (clause 5.2.15) a subscription placed at a
    /// context source names, so that the source notifies this broker in plain
    /// JSON.
    #[must_use]
    pub fn notification_endpoint(&self) -> Endpoint {
        Endpoint {
            uri: DereferenceableUri::from(self.0.clone()), // clone: the endpoint owns its URI apart from the receiver
            accept: Some(MediaTypeBuf::new(names::APPLICATION, names::JSON)),
            receiver_info: None,
            notifier_info: None,
            cooldown: None,
            timeout: None,
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::receiver::ReceiverEndpoint;
    use url::Url;

    #[test]
    fn the_receiver_hangs_below_the_advertised_origin_without_doubling_the_api_base() {
        for (advertised, expected) in [
            ("http://broker-a:1056", "http://broker-a:1056/ngsi-ld/v1/notifications"),
            ("http://broker-a:1056/", "http://broker-a:1056/ngsi-ld/v1/notifications"),
            ("http://broker-a:1056/ngsi-ld/v1", "http://broker-a:1056/ngsi-ld/v1/notifications"),
            ("https://edge.example.org/prefix", "https://edge.example.org/prefix/ngsi-ld/v1/notifications"),
        ] {
            let endpoint = ReceiverEndpoint::from_advertised(&Url::parse(advertised).expect("valid URL")).expect("receiver");
            pretty_assertions::assert_eq!(endpoint.url().as_str(), expected, "{advertised}");
        }
    }

    /// Clause 5.8.1.4: a copy at a source notifies this broker's receiver and
    /// accepts plain JSON.
    #[test]
    fn the_notification_endpoint_names_the_receiver_and_accepts_plain_json() {
        let receiver = ReceiverEndpoint::from_advertised(&Url::parse("http://broker-a:1056").expect("valid URL")).expect("receiver");
        let endpoint = receiver.notification_endpoint();
        pretty_assertions::assert_eq!(endpoint.uri.as_str(), "http://broker-a:1056/ngsi-ld/v1/notifications");
        pretty_assertions::assert_eq!(endpoint.accept.map(|accept| accept.to_string()), Some("application/json".to_owned()));
    }

    #[test]
    fn a_non_hierarchical_origin_yields_no_receiver() {
        assert!(ReceiverEndpoint::from_advertised(&Url::parse("mailto:ops@example.org").expect("valid URL")).is_none());
    }
}
