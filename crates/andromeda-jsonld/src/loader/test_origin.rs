//! A context host for loader tests, staged on the workspace's programmable
//! mock server.
//!
//! Each path answers with a fixed status, header set and body, so a test can
//! stage exactly the responses a context host could send (a redirect, an HTML
//! page naming an alternate) without reaching the network.

use andromeda_test_mocks::{
    callback_path::CallbackPath,
    callback_server::{CallbackServer, MockResponse, RequestMatcher},
    server_config::MockServerConfig,
};
use iri_rs::IriBuf;
use reqwest::{
    Method,
    StatusCode,
    header::{CONTENT_TYPE, HeaderMap, HeaderValue, LINK, LOCATION},
};
use std::net::{Ipv4Addr, SocketAddr};
use url::Host;

/// A local origin server answering staged responses.
pub(crate) struct TestOrigin {
    server: CallbackServer,
}

impl TestOrigin {
    /// Bind an origin on an ephemeral loopback port.
    pub(crate) async fn start() -> TestOrigin {
        let config = MockServerConfig::new(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)), Host::Ipv4(Ipv4Addr::LOCALHOST));
        TestOrigin {
            server: CallbackServer::start(config).await.expect("a loopback origin binds"),
        }
    }

    /// Answer every `GET path` with `response`.
    pub(crate) async fn stage(&self, path: &str, response: MockResponse) {
        self.server.enqueue(RequestMatcher::new(Method::GET, callback_path(path)), response).await;
    }

    /// The absolute URL of `path` on this origin.
    pub(crate) fn url(&self, path: &str) -> IriBuf {
        let url = self.server.endpoint(&callback_path(path)).expect("a path joins onto the origin");
        IriBuf::new(String::from(url)).expect("an origin URL is an IRI")
    }

    /// How many times `path` was requested.
    pub(crate) fn requests(&self, path: &str) -> usize {
        self.server.request_count(&RequestMatcher::new(Method::GET, callback_path(path)))
    }
}

/// A `200 OK` carrying `body` as `content_type`, with one `Link` header line
/// per entry of `links`.
pub(crate) fn ok(content_type: &str, body: &str, links: &[&str]) -> MockResponse {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, header_value(content_type));
    for link in links {
        headers.append(LINK, header_value(link));
    }
    MockResponse::builder()
        .status(StatusCode::OK)
        .headers(headers)
        .body(body.to_owned().into())
        .build()
}

/// A `200 OK` carrying `body` with no `Content-Type` at all.
pub(crate) fn untyped(body: &str) -> MockResponse {
    MockResponse::builder().status(StatusCode::OK).body(body.to_owned().into()).build()
}

/// A `302 Found` pointing at `location`.
pub(crate) fn redirect(location: &str) -> MockResponse {
    let mut headers = HeaderMap::new();
    headers.insert(LOCATION, header_value(location));
    MockResponse::builder().status(StatusCode::FOUND).headers(headers).build()
}

fn callback_path(path: &str) -> CallbackPath {
    CallbackPath::parse(path).expect("a slash-prefixed path")
}

fn header_value(value: &str) -> HeaderValue {
    HeaderValue::from_str(value).expect("a visible-ASCII header value")
}
