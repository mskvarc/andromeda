//! A context host for loader tests, staged on a local mock server.
//!
//! Each path answers with a fixed status, header set and body, so a test can
//! stage exactly the responses a context host could send (a redirect, an HTML
//! page naming an alternate) without reaching the network.

use crate::loader::context_client::ContextClient;
use andromeda_outbound_http::address_policy::AddressPolicy;
use iri_rs::IriBuf;
use reqwest::{
    StatusCode,
    header::{LINK, LOCATION},
};
use std::time::Duration;
use wiremock::{
    Mock,
    MockServer,
    ResponseTemplate,
    matchers::{method, path},
};

/// A local origin server answering staged responses.
pub(crate) struct TestOrigin {
    /// The mock server the staged responses are mounted on.
    server: MockServer,
}

impl TestOrigin {
    /// Bind an origin on an ephemeral loopback port.
    pub(crate) async fn start() -> TestOrigin {
        TestOrigin {
            server: MockServer::start().await,
        }
    }

    /// Answer every `GET path` with `response`.
    pub(crate) async fn stage(&self, served: &str, response: ResponseTemplate) {
        Mock::given(method("GET")).and(path(served)).respond_with(response).mount(&self.server).await;
    }

    /// The absolute URL of `path` on this origin.
    pub(crate) fn url(&self, served: &str) -> IriBuf {
        IriBuf::new(format!("{}{served}", self.server.uri())).expect("an origin URL is an IRI")
    }

    /// The port the origin listens on, for a URL naming another loopback
    /// address on the same port.
    pub(crate) fn port(&self) -> u16 {
        self.server.address().port()
    }

    /// How many times `path` was requested.
    pub(crate) async fn requests(&self, served: &str) -> usize {
        self.server
            .received_requests()
            .await
            .expect("the origin records its requests")
            .iter()
            .filter(|request| request.method.as_str() == "GET" && request.url.path() == served)
            .count()
    }
}

/// A context client admitting what `addresses` admits, with the download
/// limits of a default loader.
pub(crate) fn client(addresses: AddressPolicy) -> ContextClient {
    ContextClient::new(addresses, Duration::from_secs(30), 5).expect("a buildable context client")
}

/// A `200 OK` carrying `body` as `content_type`, with one `Link` header line
/// per entry of `links`.
pub(crate) fn ok(content_type: &str, body: &str, links: &[&str]) -> ResponseTemplate {
    links
        .iter()
        .fold(ResponseTemplate::new(StatusCode::OK), |response, link| response.append_header(LINK, *link))
        .set_body_raw(body, content_type)
}

/// A `200 OK` carrying `body` with no `Content-Type` at all.
pub(crate) fn untyped(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(StatusCode::OK).set_body_bytes(body)
}

/// A `302 Found` pointing at `location`.
pub(crate) fn redirect(location: &str) -> ResponseTemplate {
    ResponseTemplate::new(StatusCode::FOUND).insert_header(LOCATION, location)
}
