//! A `receiverInfo` entry read as a header field of an HTTP notification.
//!
//! ETSI GS CIM 009 clauses 6.3.8 and 6.3.9 send each `receiverInfo` entry of an
//! `http` or `https` endpoint as a custom header of the notification, and
//! clause 5.16.6 sends the status notification of a Snapshot the same way. The
//! clauses ask only that the key and the value adhere to IETF RFC 7230, which
//! [`HttpHeaderField`] checks, and reserve no name. The notification itself has
//! to stay a well-formed message, though. Its media type is the one
//! `endpoint.accept` selects, its length and framing are what the HTTP client
//! computes for the body it writes, and the fields of the connection belong to
//! that client. An entry naming one of those would send a second
//! `Content-Type`, a length that does not match the body, two framings at once
//! or a field the client manages itself, so such an entry is refused. Admission
//! and delivery read every entry through [`notification_header_field`], so what
//! is admitted is exactly what is sent.

use crate::{
    http_header_field::{HttpHeaderField, HttpHeaderFieldError},
    key_value_pair::KeyValuePair,
};
use http::{
    HeaderName,
    header::{CONNECTION, CONTENT_LENGTH, CONTENT_TYPE, HOST, TE, TRAILER, TRANSFER_ENCODING, UPGRADE},
};
use strum::{Display, EnumIter, IntoEnumIterator};
use thiserror::Error;

/// The `Keep-Alive` field name, which has no constant of its own in `http`.
const KEEP_ALIVE: HeaderName = HeaderName::from_static("keep-alive");

/// A header field the broker and its HTTP client write on every notification,
/// which no `receiverInfo` entry may name. Its `Display` is the field name in
/// its registered spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumIter)]
pub enum ReservedHeader {
    /// `Content-Type`, the media type `endpoint.accept` selects for the body.
    #[strum(serialize = "Content-Type")]
    ContentType,
    /// `Content-Length`, the length of the body the client writes.
    #[strum(serialize = "Content-Length")]
    ContentLength,
    /// `Transfer-Encoding`, the framing the client chooses for the body.
    #[strum(serialize = "Transfer-Encoding")]
    TransferEncoding,
    /// `Host`, the authority of the endpoint URI.
    #[strum(serialize = "Host")]
    Host,
    /// `Connection`, the options of the connection the client holds.
    #[strum(serialize = "Connection")]
    Connection,
    /// `TE`, the transfer codings the client accepts in the answer.
    #[strum(serialize = "TE")]
    Te,
    /// `Trailer`, the fields that follow a chunked body.
    #[strum(serialize = "Trailer")]
    Trailer,
    /// `Upgrade`, a switch of the client's connection to another protocol.
    #[strum(serialize = "Upgrade")]
    Upgrade,
    /// `Keep-Alive`, how long the client's connection persists.
    #[strum(serialize = "Keep-Alive")]
    KeepAlive,
}

impl ReservedHeader {
    /// The field name as a header map holds it, in lower case.
    #[must_use]
    pub const fn name(self) -> HeaderName {
        match self {
            ReservedHeader::ContentType => CONTENT_TYPE,
            ReservedHeader::ContentLength => CONTENT_LENGTH,
            ReservedHeader::TransferEncoding => TRANSFER_ENCODING,
            ReservedHeader::Host => HOST,
            ReservedHeader::Connection => CONNECTION,
            ReservedHeader::Te => TE,
            ReservedHeader::Trailer => TRAILER,
            ReservedHeader::Upgrade => UPGRADE,
            ReservedHeader::KeepAlive => KEEP_ALIVE,
        }
    }
}

/// Why a `receiverInfo` entry cannot be a header field of an HTTP notification.
#[derive(Debug, Error)]
pub enum NotificationHeaderFieldError {
    /// The entry is not an HTTP header field (IETF RFC 7230 section 3.2).
    #[error(transparent)]
    Field(#[from] HttpHeaderFieldError),
    /// The key names a field the broker writes on every notification itself.
    #[error("the key names the {0} header, which the broker writes on every notification itself")]
    Reserved(ReservedHeader),
}

/// `pair` read as a header field of an HTTP notification.
///
/// # Errors
///
/// Returns [`NotificationHeaderFieldError::Field`] when `pair` breaks IETF RFC
/// 7230, and [`NotificationHeaderFieldError::Reserved`] when its key names a
/// [`ReservedHeader`], whatever the letter case of the key.
pub fn notification_header_field(pair: &KeyValuePair) -> Result<HttpHeaderField, NotificationHeaderFieldError> {
    let field = HttpHeaderField::try_from(pair)?;
    match ReservedHeader::iter().find(|reserved| reserved.name() == field.name) {
        Some(reserved) => Err(NotificationHeaderFieldError::Reserved(reserved)),
        None => Ok(field),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        http_header_field::HttpHeaderFieldError,
        key_value_pair::KeyValuePair,
        notification_header_field::{NotificationHeaderFieldError, ReservedHeader, notification_header_field},
    };

    fn pair(key: &str, value: &str) -> KeyValuePair {
        KeyValuePair {
            key: key.into(),
            value: value.into(),
        }
    }

    /// Generates one test per reserved field name, asserting that the key is
    /// refused as that [`ReservedHeader`] in its registered spelling, in lower
    /// case and in upper case.
    macro_rules! refused_in_any_letter_case {
        ($($test:ident: $spelling:literal => $reserved:ident),+ $(,)?) => {
            $(
                #[test]
                fn $test() {
                    for key in [$spelling.to_owned(), $spelling.to_ascii_lowercase(), $spelling.to_ascii_uppercase()] {
                        let refused = notification_header_field(&pair(&key, "1"));
                        assert!(
                            matches!(refused, Err(NotificationHeaderFieldError::Reserved(ReservedHeader::$reserved))),
                            "{key}: {refused:?}"
                        );
                    }
                }
            )+
        };
    }

    refused_in_any_letter_case! {
        content_type_is_refused_in_any_letter_case: "Content-Type" => ContentType,
        content_length_is_refused_in_any_letter_case: "Content-Length" => ContentLength,
        transfer_encoding_is_refused_in_any_letter_case: "Transfer-Encoding" => TransferEncoding,
        host_is_refused_in_any_letter_case: "Host" => Host,
        connection_is_refused_in_any_letter_case: "Connection" => Connection,
        te_is_refused_in_any_letter_case: "TE" => Te,
        trailer_is_refused_in_any_letter_case: "Trailer" => Trailer,
        upgrade_is_refused_in_any_letter_case: "Upgrade" => Upgrade,
        keep_alive_is_refused_in_any_letter_case: "Keep-Alive" => KeepAlive,
    }

    #[test]
    fn a_custom_header_becomes_a_header_field() {
        let field = notification_header_field(&pair("Authorization", "Bearer abc"));
        assert!(matches!(field, Ok(field) if field.name == "authorization" && field.value == "Bearer abc"));
    }

    /// Delivery joins a `Link` entry to the `@context` link rather than
    /// sending it beside a field of the broker's own.
    #[test]
    fn a_link_entry_becomes_a_header_field() {
        assert!(notification_header_field(&pair("Link", "<https://example.org/doc>; rel=\"describedby\"")).is_ok());
    }

    #[test]
    fn a_key_that_merely_starts_with_a_reserved_name_becomes_a_header_field() {
        assert!(notification_header_field(&pair("Content-Type-Hint", "x")).is_ok());
    }

    #[test]
    fn a_pair_that_is_no_header_field_is_refused_as_such() {
        assert!(matches!(
            notification_header_field(&pair("Bad Name", "x")),
            Err(NotificationHeaderFieldError::Field(HttpHeaderFieldError::Name(_)))
        ));
        assert!(matches!(
            notification_header_field(&pair("X-A", "a\nb")),
            Err(NotificationHeaderFieldError::Field(HttpHeaderFieldError::Value(_)))
        ));
    }

    #[test]
    fn a_reserved_name_is_displayed_in_its_registered_spelling() {
        pretty_assertions::assert_eq!(ReservedHeader::Te.to_string(), "TE");
        pretty_assertions::assert_eq!(ReservedHeader::KeepAlive.to_string(), "Keep-Alive");
        pretty_assertions::assert_eq!(ReservedHeader::ContentLength.to_string(), "Content-Length");
    }
}
