//! What a broker's answer to an upload it refuses before reading the content says of the connection: over HTTP/1.1 it
//! announces the close (RFC 9110 section 10.1.1), the only way there to stop a client that is still sending (RFC 9112
//! section 9.6), and over HTTP/2 it carries no `Connection` field, which RFC 9113 section 8.2.2 forbids, since ending
//! the stream stops the upload.
//!
//! Every refused upload goes to the TLS broker, which offers both versions in ALPN.

use crate::{
    broker::BrokerClient,
    config::BrokerRole,
    problem::AnsweredMatcher,
    tls_connection::{Connection, HttpVersion, RawClient, TlsError},
};
use http::{Version, header::CONNECTION};
use ngsi_ld::{client::ClientError, http::ResponseHead};
use strum::EnumIter;

/// The octets of content a refused upload carries: far more than the socket buffers hold, so the client is still
/// sending when the refusal arrives.
pub const REFUSED_UPLOAD: u64 = 16 * 1024 * 1024;

/// An HTTP version over TCP that an upload is refused over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter)]
pub enum RefusedVersion {
    /// HTTP/1.1 only.
    Http11,
    /// HTTP/2 only.
    Http2,
}

impl RefusedVersion {
    /// How a client speaking this version alone reaches the TLS broker.
    fn connection(self) -> Connection {
        let offered = match self {
            RefusedVersion::Http11 => HttpVersion::Http11,
            RefusedVersion::Http2 => HttpVersion::Http2,
        };
        Connection::new().trusting("tls/ca.crt").offering(offered)
    }

    /// A client of the TLS broker speaking this version alone.
    ///
    /// # Errors
    ///
    /// Returns [`TlsError`] when the stack names no TLS broker or the client cannot be built.
    pub async fn client(self) -> Result<BrokerClient, TlsError> {
        self.connection().client(BrokerRole::Tls).await
    }

    /// A plain HTTP client of the TLS broker speaking this version alone, for an upload the typed client cannot send.
    ///
    /// # Errors
    ///
    /// Returns [`TlsError`] when the stack names no TLS broker or the client cannot be built.
    pub async fn raw_client(self) -> Result<RawClient, TlsError> {
        self.connection().raw_client(BrokerRole::Tls).await
    }

    /// The version the answer arrives in.
    #[must_use]
    pub const fn version(self) -> Version {
        match self {
            RefusedVersion::Http11 => Version::HTTP_11,
            RefusedVersion::Http2 => Version::HTTP_2,
        }
    }

    /// The `Connection` field the refusal carries under this version.
    #[must_use]
    pub const fn connection_field(self) -> Option<&'static str> {
        match self {
            RefusedVersion::Http11 => Some("close"),
            RefusedVersion::Http2 => None,
        }
    }

    /// `matcher`, also requiring the `Connection` field the refusal carries under this version.
    #[must_use]
    pub fn announcing(self, matcher: AnsweredMatcher) -> AnsweredMatcher {
        match self.connection_field() {
            Some(field) => matcher.with_header(CONNECTION, field),
            None => matcher.without_header(CONNECTION),
        }
    }
}

/// The version an error answer arrived in, when the broker answered.
#[must_use]
pub fn answered_version<V, E>(answer: &Result<V, ClientError<E>>) -> Option<Version> {
    answer.as_ref().err().and_then(ClientError::head).map(ResponseHead::version)
}

#[cfg(test)]
mod tests {
    use crate::{early_refusal::RefusedVersion, problem::answered};
    use bytes::Bytes;
    use googletest::prelude::*;
    use http::{Response, StatusCode, Version, header::CONNECTION};
    use ngsi_ld::client::ClientError;
    use std::{convert::Infallible, result::Result as StdResult};
    use strum::IntoEnumIterator;

    /// A 404 answer carrying `connection`, if any, as its `Connection` field.
    fn not_found(connection: Option<&'static str>) -> StdResult<StdResult<Response<Bytes>, ClientError<Infallible>>, http::Error> {
        let builder = Response::builder().status(StatusCode::NOT_FOUND);
        let builder = match connection {
            Some(value) => builder.header(CONNECTION, value),
            None => builder,
        };
        Ok(Ok(builder.body(Bytes::new())?))
    }

    #[gtest]
    fn an_http11_refusal_announces_the_close_and_an_http2_one_carries_no_field() {
        let fields: Vec<_> = RefusedVersion::iter().map(|version| (version.version(), version.connection_field())).collect();
        expect_that!(fields, elements_are![eq(&(Version::HTTP_11, Some("close"))), eq(&(Version::HTTP_2, None))]);
    }

    #[gtest]
    fn an_http11_refusal_matches_only_with_the_close_announced() -> Result<()> {
        let announcing = RefusedVersion::Http11.announcing(answered(StatusCode::NOT_FOUND));
        expect_that!(&not_found(Some("close"))?, announcing.clone());
        expect_that!(&not_found(Some("keep-alive"))?, not(announcing.clone()));
        verify_that!(&not_found(None)?, not(announcing))
    }

    #[gtest]
    fn an_http2_refusal_matches_only_without_a_connection_field() -> Result<()> {
        let announcing = RefusedVersion::Http2.announcing(answered(StatusCode::NOT_FOUND));
        expect_that!(&not_found(None)?, announcing.clone());
        verify_that!(&not_found(Some("close"))?, not(announcing))
    }
}
