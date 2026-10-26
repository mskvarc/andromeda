//! Admitting a Snapshot's `receiverInfo` against its endpoint.
//!
//! Clause 5.16.6 sends the status notification "in the way required for the
//! binding protocol", and Table 5.2.22-1 makes `receiverInfo` binding
//! dependent. An `http` or `https` endpoint reads each entry as an HTTP header
//! field (IETF RFC 7230), so a malformed entry is refused here instead of being
//! dropped at delivery.

use crate::{error::SnapshotError, snapshot_validation_reason::SnapshotValidationReason};
use andromeda_ngsi_ld_model::{http_header_field::HttpHeaderField, key_value_pair::KeyValuePair};
use url::Url;

/// Reject a `receiverInfo` entry that is not an HTTP header field when the
/// endpoint is an `http` or `https` URL. Any other endpoint, or none, leaves
/// `receiverInfo` unchecked.
///
/// # Errors
///
/// Returns [`SnapshotError::Validation`] naming the first entry that breaks
/// IETF RFC 7230.
pub fn validate_receiver_info(endpoint: Option<&Url>, receiver_info: Option<&[KeyValuePair]>) -> Result<(), SnapshotError> {
    let Some(endpoint) = endpoint else {
        return Ok(());
    };
    if !matches!(endpoint.scheme(), "http" | "https") {
        return Ok(());
    }
    receiver_info
        .unwrap_or_default()
        .iter()
        .try_for_each(|pair| HttpHeaderField::try_from(pair).map(|_field| ()))
        .map_err(|error| SnapshotValidationReason::ReceiverInfo(error).into())
}

#[cfg(test)]
mod tests {
    use crate::{error::SnapshotError, receiver_info_admission::validate_receiver_info, snapshot_validation_reason::SnapshotValidationReason};
    use andromeda_ngsi_ld_model::{http_header_field::HttpHeaderFieldError, key_value_pair::KeyValuePair};
    use url::Url;

    fn endpoint(raw: &str) -> Option<Url> {
        Url::parse(raw).ok()
    }

    fn pair(key: &str, value: &str) -> KeyValuePair {
        KeyValuePair {
            key: key.into(),
            value: value.into(),
        }
    }

    #[test]
    fn a_bad_key_on_an_http_endpoint_is_refused() {
        let refused = validate_receiver_info(endpoint("http://example.com/s").as_ref(), Some(&[pair("Bad Name", "x")]));
        assert!(matches!(
            refused,
            Err(SnapshotError::Validation(SnapshotValidationReason::ReceiverInfo(HttpHeaderFieldError::Name(_))))
        ));
    }

    #[test]
    fn a_bad_value_on_an_https_endpoint_is_refused() {
        let refused = validate_receiver_info(endpoint("https://example.com/s").as_ref(), Some(&[pair("X-A", "a\nb")]));
        assert!(matches!(
            refused,
            Err(SnapshotError::Validation(SnapshotValidationReason::ReceiverInfo(HttpHeaderFieldError::Value(
                _
            ))))
        ));
    }

    #[test]
    fn the_same_entry_on_an_mqtt_endpoint_is_accepted() {
        assert!(validate_receiver_info(endpoint("mqtt://broker:1883/t").as_ref(), Some(&[pair("Bad Name", "x")])).is_ok());
    }

    #[test]
    fn a_good_entry_on_an_http_endpoint_is_accepted() {
        assert!(validate_receiver_info(endpoint("http://example.com/s").as_ref(), Some(&[pair("Authorization", "Bearer abc")])).is_ok());
    }

    #[test]
    fn a_bad_entry_without_an_endpoint_is_left_alone() {
        assert!(validate_receiver_info(None, Some(&[pair("Bad Name", "x")])).is_ok());
    }
}
