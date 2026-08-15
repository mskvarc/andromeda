//! Typed `Accept` request header for outbound JSON-LD context retrieval.
//!
//! JSON-LD 1.1 API clause 6.1 requires a context request to advertise
//! `application/ld+json` and to keep `application/json` acceptable, because a
//! context is frequently served under the plain JSON type. That negotiation set
//! is fixed rather than caller-chosen, so the header carries no value: the type
//! itself is the offer.
//!
//! It is distinct from [`Accept`](crate::accept::Accept), which negotiates the
//! NGSI-LD API's own response formats and is quality-ranked. This one only ever
//! travels outbound, on the broker's own requests for remote contexts.

use crate::media_type::ResponseMediaType;
use http::{HeaderName, HeaderValue};
use mediatype::MediaTypeList;
use std::iter;

/// The lowercase `Accept` field name this header is sent under.
static ACCEPT: HeaderName = HeaderName::from_static("accept");

/// The response media types a context request offers, most preferred first.
const OFFERED: [ResponseMediaType; 2] = [ResponseMediaType::JsonLd, ResponseMediaType::Json];

/// The fixed negotiation set used when requesting a remote JSON-LD context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JsonLdAccept;

impl headers::Header for JsonLdAccept {
    fn name() -> &'static HeaderName {
        &ACCEPT
    }

    fn decode<'i, I>(values: &mut I) -> Result<JsonLdAccept, headers::Error>
    where
        I: Iterator<Item = &'i HeaderValue>,
    {
        let value = values.next().ok_or_else(headers::Error::invalid)?;
        let text = value.to_str().map_err(|_| headers::Error::invalid())?;
        let offered = MediaTypeList::new(text)
            .map(|entry| entry.ok().and_then(|media_type| ResponseMediaType::from_media_type(&media_type)))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(headers::Error::invalid)?;
        (offered == OFFERED).then_some(JsonLdAccept).ok_or_else(headers::Error::invalid)
    }

    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        let offer = OFFERED.map(|media_type| media_type.as_str()).join(", ");
        match HeaderValue::from_str(&offer) {
            Ok(value) => values.extend(iter::once(value)),
            Err(error) => tracing::warn!(error = %error, "failed to encode the JSON-LD context Accept header"),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::json_ld_accept::JsonLdAccept;
    use headers::Header;
    use http::HeaderValue;
    use std::iter;

    /// Encode the header to its single wire value.
    fn encoded() -> HeaderValue {
        let mut values = Vec::new();
        JsonLdAccept.encode(&mut values);
        values.into_iter().next().unwrap_or_else(|| unreachable!("the offer is a valid header value"))
    }

    #[test]
    fn encoding_offers_ld_json_ahead_of_plain_json() {
        pretty_assertions::assert_eq!(encoded(), HeaderValue::from_static("application/ld+json, application/json"));
    }

    #[test]
    fn the_encoded_offer_decodes_back_to_the_same_header() {
        let value = encoded();
        let mut values = iter::once(&value);

        pretty_assertions::assert_eq!(JsonLdAccept::decode(&mut values).expect("the encoded offer decodes"), JsonLdAccept);
    }

    #[test]
    fn decoding_rejects_a_narrower_negotiation_set() {
        let value = HeaderValue::from_static("application/json");
        let mut values = iter::once(&value);

        assert!(JsonLdAccept::decode(&mut values).is_err());
    }

    #[test]
    fn decoding_rejects_the_offer_in_the_opposite_preference_order() {
        let value = HeaderValue::from_static("application/json, application/ld+json");
        let mut values = iter::once(&value);

        assert!(JsonLdAccept::decode(&mut values).is_err());
    }

    #[test]
    fn decoding_rejects_an_unrecognised_media_type_in_the_set() {
        let value = HeaderValue::from_static("application/ld+json, application/json, text/html");
        let mut values = iter::once(&value);

        assert!(JsonLdAccept::decode(&mut values).is_err());
    }

    #[test]
    fn decoding_rejects_an_absent_field() {
        let mut values = iter::empty();

        assert!(JsonLdAccept::decode(&mut values).is_err());
    }
}
