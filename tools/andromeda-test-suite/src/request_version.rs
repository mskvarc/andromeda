//! A transport that sends every request under the HTTP version its client speaks.
//!
//! The `ngsi-ld` client writes no HTTP version on the requests it builds, so each carries HTTP/1.1, the default of an
//! HTTP request; over TCP the connection's ALPN decides the version actually spoken. `reqwest` takes the QUIC path of
//! a client built for HTTP/3 only for a request whose version is HTTP/3, so a client speaking HTTP/3 sets it on every
//! request.

use bytes::Bytes;
use http::{Request, Response, Version};
use ngsi_ld::client::Transport;

/// A transport setting the HTTP version of every request it carries, or leaving it as written.
#[derive(Debug, Clone)]
pub struct RequestVersion<T> {
    /// The transport that sends.
    inner: T,
    /// The version set on every request; `None` leaves each request's version as written.
    version: Option<Version>,
}

impl<T: Transport> RequestVersion<T> {
    /// Sends every request through `inner` with its version as written.
    #[must_use]
    pub const fn as_written(inner: T) -> RequestVersion<T> {
        RequestVersion { inner, version: None }
    }

    /// Sends every request through `inner` under `version`.
    #[must_use]
    pub const fn pinned(inner: T, version: Version) -> RequestVersion<T> {
        RequestVersion { inner, version: Some(version) }
    }
}

impl<T: Transport> Transport for RequestVersion<T> {
    type Error = T::Error;

    async fn send(&self, request: Request<Bytes>) -> Result<Response<Bytes>, T::Error> {
        let mut request = request;
        if let Some(version) = self.version {
            *request.version_mut() = version;
        }
        let request = request;
        self.inner.send(request).await
    }
}

#[cfg(test)]
mod tests {
    use crate::request_version::RequestVersion;
    use bytes::Bytes;
    use googletest::prelude::*;
    use http::{Request, Response, Version};
    use ngsi_ld::client::Transport;
    use std::{
        convert::Infallible,
        future::{self, Future},
        result,
    };

    /// Answers every request with an empty response under the request's own version.
    struct Echo;

    impl Transport for Echo {
        type Error = Infallible;

        fn send(&self, request: Request<Bytes>) -> impl Future<Output = result::Result<Response<Bytes>, Infallible>> + Send {
            let mut response = Response::new(Bytes::new());
            *response.version_mut() = request.version();
            future::ready(Ok(response))
        }
    }

    #[gtest]
    #[tokio::test]
    async fn a_pinned_version_replaces_the_version_a_request_was_written_with() -> Result<()> {
        let response = RequestVersion::pinned(Echo, Version::HTTP_3).send(Request::new(Bytes::new())).await?;
        verify_that!(response.version(), eq(Version::HTTP_3))
    }

    #[gtest]
    #[tokio::test]
    async fn a_request_keeps_its_version_when_none_is_pinned() -> Result<()> {
        let response = RequestVersion::as_written(Echo).send(Request::new(Bytes::new())).await?;
        verify_that!(response.version(), eq(Version::HTTP_11))
    }
}
