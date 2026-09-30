//! An origin server the broker downloads remote `@context` documents from.
//!
//! Every test that names a remote context serves it from its own origin, bound
//! to an ephemeral port on the host the broker reaches the test process by, so
//! no case ever touches the network and no two tests share a URL. A path
//! answers its last staged response for as long as the origin runs.

use andromeda_test_mocks::{
    callback_path::CallbackPath,
    callback_server::{CallbackServer, MockResponse, RequestMatcher},
    server_config::MockServerConfig,
};
use axum::{
    body::Bytes,
    http::{
        HeaderMap,
        HeaderName,
        HeaderValue,
        Method,
        StatusCode,
        header::{CONTENT_TYPE, LOCATION},
    },
};
use serde_json::Value;

/// One response the origin answers a path with.
pub struct OriginResponse {
    status: StatusCode,
    content_type: Option<&'static str>,
    headers: Vec<(HeaderName, String)>,
    body: String,
}

impl OriginResponse {
    /// A 200 carrying `body` as `content_type`, or with no `Content-Type` at
    /// all.
    pub fn document(content_type: Option<&'static str>, body: impl Into<String>) -> OriginResponse {
        OriginResponse {
            status: StatusCode::OK,
            content_type,
            headers: Vec::new(),
            body: body.into(),
        }
    }

    /// A 302 to `location`.
    pub fn redirect(location: String) -> OriginResponse {
        OriginResponse {
            status: StatusCode::FOUND,
            content_type: None,
            headers: vec![(LOCATION, location)],
            body: String::new(),
        }
    }

    /// The same response with one more header.
    pub fn with_header(mut self, name: HeaderName, value: impl Into<String>) -> OriginResponse {
        self.headers.push((name, value.into()));
        self
    }
}

/// An origin serving `@context` documents.
pub struct ContextOrigin {
    server: CallbackServer,
    base: String,
}

impl ContextOrigin {
    /// Start an origin on an ephemeral port.
    pub async fn start() -> ContextOrigin {
        let server = CallbackServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
            .await
            .expect("the origin server starts");
        let base = server
            .endpoint(&CallbackPath::parse("/".to_owned()).expect("an absolute path"))
            .expect("an endpoint")
            .to_string()
            .trim_end_matches('/')
            .to_owned();
        ContextOrigin { server, base }
    }

    /// The absolute URL of `path` on this origin.
    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    /// Serve `document` at `path` as `application/ld+json`, returning its URL.
    pub async fn serve_document(&self, path: &str, document: &Value) -> String {
        self.serve(path, OriginResponse::document(Some("application/ld+json"), document.to_string()))
            .await
    }

    /// Answer `path` with `response`, returning the path's URL.
    pub async fn serve(&self, path: &str, response: OriginResponse) -> String {
        let mut headers = HeaderMap::new();
        if let Some(content_type) = response.content_type {
            headers.insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
        }
        for (name, value) in response.headers {
            headers.append(name, HeaderValue::from_str(&value).expect("a header value"));
        }
        self.server
            .enqueue(
                RequestMatcher::new(Method::GET, CallbackPath::parse(path.to_owned()).expect("an absolute path")),
                MockResponse::builder()
                    .status(response.status)
                    .headers(headers)
                    .body(Bytes::from(response.body))
                    .build(),
            )
            .await;
        self.url(path)
    }

    /// How many times the broker fetched `path`.
    pub fn fetch_count(&self, path: &str) -> usize {
        self.server.request_count(&RequestMatcher::new(
            Method::GET,
            CallbackPath::parse(path.to_owned()).expect("an absolute path"),
        ))
    }

    /// Stop answering.
    pub async fn shutdown(self) {
        self.server.shutdown().await.expect("origin shutdown");
    }
}
