//! One request of a test: an HTTP request to a broker or to the test's stub server, or a call on the stub server's
//! stubs and journal, with how often it is sent.
//!
//! The URL, header values, query parameters and bodies are templates, filled from the test's variables when sent.

use crate::sequence::{
    body::{Body, Document},
    payload::Payload,
};
use http::Method;
use std::time::Duration;

/// The pause between two attempts when a retried request names none.
const DEFAULT_RETRY_INTERVAL: Duration = Duration::from_secs(1);

/// What a request does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// An HTTP request.
    Http(Box<HttpRequest>),
    /// Sets a stub, the JSON template of a stub mapping.
    SetStub(String),
    /// Removes the stub with the identifier, a template.
    RemoveStub(String),
    /// Removes the stubs a metadata pattern selects, a JSON template.
    RemoveStubs(String),
    /// Reads the requests the stub server received that match a request pattern, a JSON template.
    FindRequests(String),
    /// Counts the requests the stub server received that match a request pattern, a JSON template.
    CountRequests(String),
}

/// An HTTP request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    /// The method.
    pub method: Method,
    /// The URL.
    pub url: String,
    /// The header fields, in order.
    pub headers: Vec<(String, String)>,
    /// The query parameters added to the URL, in order, percent-encoded when sent.
    pub query: Vec<(String, String)>,
    /// The body.
    pub body: Body,
    /// Whether the query parameters are read by the operation's parameter model before sending.
    pub parameters: Parameters,
    /// How the client sending it connects.
    pub connection: Connection,
}

/// How the client that sends a request connects: the authority it trusts beside the platform's, the identity it
/// presents, and the HTTP version it speaks.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Connection {
    /// The PEM file of a certificate authority the client trusts, a template.
    pub authority: Option<String>,
    /// The PEM files of the certificate and key the client presents, templates.
    pub identity: Option<(String, String)>,
    /// The HTTP version.
    pub version: HttpVersion,
}

/// The HTTP version a client speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HttpVersion {
    /// HTTP/2 when TLS negotiates it, HTTP/1.1 otherwise.
    #[default]
    Negotiated,
    /// HTTP/2 offered first over TLS: what the client negotiates anyway, named by a test whose subject it is.
    Http2,
    /// HTTP/1.1 only.
    Http11,
}

/// Whether a request's query parameters are read by the typed parameter model before sending.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parameters {
    /// Read by the model of the operation, when the crate has one.
    Typed,
    /// Sent as written: the model refuses them.
    Unchecked,
}

/// How often a request is sent until its answer meets the expectation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retry {
    /// The attempts after the first.
    pub count: u32,
    /// The pause between two attempts.
    pub interval: Duration,
}

/// A request and how often it is sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// What it does.
    pub action: Action,
    /// Whether it is sent again until its answer meets the expectation.
    pub retry: Option<Retry>,
    /// How many times it is sent, each answer checked.
    pub repeat: u32,
}

impl Request {
    /// A request doing `action` once.
    const fn of(action: Action) -> Request {
        Request {
            action,
            retry: None,
            repeat: 1,
        }
    }

    /// An HTTP request with `method` to the URL template `url`.
    #[must_use]
    pub fn http(method: Method, url: &str) -> Request {
        Request::of(Action::Http(Box::new(HttpRequest {
            method,
            url: url.to_owned(),
            headers: Vec::new(),
            query: Vec::new(),
            body: Body::Empty,
            parameters: Parameters::Typed,
            connection: Connection::default(),
        })))
    }

    /// A GET request.
    #[must_use]
    pub fn get(url: &str) -> Request {
        Request::http(Method::GET, url)
    }

    /// A POST request.
    #[must_use]
    pub fn post(url: &str) -> Request {
        Request::http(Method::POST, url)
    }

    /// A PUT request.
    #[must_use]
    pub fn put(url: &str) -> Request {
        Request::http(Method::PUT, url)
    }

    /// A PATCH request.
    #[must_use]
    pub fn patch(url: &str) -> Request {
        Request::http(Method::PATCH, url)
    }

    /// A DELETE request.
    #[must_use]
    pub fn delete(url: &str) -> Request {
        Request::http(Method::DELETE, url)
    }

    /// A HEAD request.
    #[must_use]
    pub fn head(url: &str) -> Request {
        Request::http(Method::HEAD, url)
    }

    /// An OPTIONS request.
    #[must_use]
    pub fn options(url: &str) -> Request {
        Request::http(Method::OPTIONS, url)
    }

    /// Sets the stub `mapping` on the test's stub server.
    #[must_use]
    pub fn set_stub(mapping: &str) -> Request {
        Request::of(Action::SetStub(mapping.to_owned()))
    }

    /// Removes the stub with identifier `id`.
    #[must_use]
    pub fn remove_stub(id: &str) -> Request {
        Request::of(Action::RemoveStub(id.to_owned()))
    }

    /// Removes the stubs the metadata `pattern` selects.
    #[must_use]
    pub fn remove_stubs(pattern: &str) -> Request {
        Request::of(Action::RemoveStubs(pattern.to_owned()))
    }

    /// Reads the received requests that match `pattern`.
    #[must_use]
    pub fn find_requests(pattern: &str) -> Request {
        Request::of(Action::FindRequests(pattern.to_owned()))
    }

    /// Counts the received requests that match `pattern`.
    #[must_use]
    pub fn count_requests(pattern: &str) -> Request {
        Request::of(Action::CountRequests(pattern.to_owned()))
    }

    /// The HTTP request changed by `change`; a call on the stub server is left as it is.
    fn with_http(mut self, change: impl FnOnce(&mut HttpRequest)) -> Request {
        match &mut self.action {
            Action::Http(request) => change(request),
            Action::SetStub(_) | Action::RemoveStub(_) | Action::RemoveStubs(_) | Action::FindRequests(_) | Action::CountRequests(_) => {}
        }
        self
    }

    /// Adds the header `name` with the template `value`.
    #[must_use]
    pub fn header(self, name: &str, value: &str) -> Request {
        self.with_http(|request| request.headers.push((name.to_owned(), value.to_owned())))
    }

    /// Adds the query parameter `name` with the template `value`.
    #[must_use]
    pub fn query(self, name: &str, value: &str) -> Request {
        self.with_http(|request| request.query.push((name.to_owned(), value.to_owned())))
    }

    /// Sends the query parameters without reading them with the operation's parameter model, which refuses them.
    #[must_use]
    pub fn unchecked_query(self) -> Request {
        self.with_http(|request| request.parameters = Parameters::Unchecked)
    }

    /// Sent by a client trusting the certificate authority in the PEM file at the template `path`.
    #[must_use]
    pub fn authority(self, path: &str) -> Request {
        self.with_http(|request| request.connection.authority = Some(path.to_owned()))
    }

    /// Sent by a client presenting the certificate and key in the PEM files at the templates `certificate` and `key`.
    #[must_use]
    pub fn identity(self, certificate: &str, key: &str) -> Request {
        self.with_http(|request| request.connection.identity = Some((certificate.to_owned(), key.to_owned())))
    }

    /// Sent by a client speaking HTTP `version`.
    #[must_use]
    pub fn http_version(self, version: HttpVersion) -> Request {
        self.with_http(|request| request.connection.version = version)
    }

    /// The body `body`.
    #[must_use]
    pub fn body(self, body: Body) -> Request {
        self.with_http(|request| request.body = body)
    }

    /// The typed body `document` holding the template `template`.
    fn typed(self, document: Document, template: &str) -> Request {
        self.body(Body::Typed(document, template.to_owned()))
    }

    /// An entity.
    #[must_use]
    pub fn entity(self, template: &str) -> Request {
        self.typed(Document::Entity, template)
    }

    /// An array of entities.
    #[must_use]
    pub fn entities(self, template: &str) -> Request {
        self.typed(Document::Entities, template)
    }

    /// An entity fragment.
    #[must_use]
    pub fn fragment(self, template: &str) -> Request {
        self.typed(Document::Fragment, template)
    }

    /// An array of complete entity fragments.
    #[must_use]
    pub fn complete_fragments(self, template: &str) -> Request {
        self.typed(Document::CompleteFragments, template)
    }

    /// A partial attribute update.
    #[must_use]
    pub fn attribute_patch(self, template: &str) -> Request {
        self.typed(Document::AttributePatch, template)
    }

    /// An attribute.
    #[must_use]
    pub fn attribute(self, template: &str) -> Request {
        self.typed(Document::AttributeInput, template)
    }

    /// A temporal entity.
    #[must_use]
    pub fn temporal(self, template: &str) -> Request {
        self.typed(Document::Temporal, template)
    }

    /// The attributes of a temporal entity.
    #[must_use]
    pub fn temporal_attributes(self, template: &str) -> Request {
        self.typed(Document::TemporalAttributes, template)
    }

    /// A subscription.
    #[must_use]
    pub fn subscription(self, template: &str) -> Request {
        self.typed(Document::Subscription, template)
    }

    /// A context source registration subscription.
    #[must_use]
    pub fn registration_subscription(self, template: &str) -> Request {
        self.typed(Document::RegistrationSubscription, template)
    }

    /// A context source registration.
    #[must_use]
    pub fn registration(self, template: &str) -> Request {
        self.typed(Document::Registration, template)
    }

    /// A snapshot.
    #[must_use]
    pub fn snapshot(self, template: &str) -> Request {
        self.typed(Document::Snapshot, template)
    }

    /// An entity query.
    #[must_use]
    pub fn entity_query(self, template: &str) -> Request {
        self.typed(Document::EntityQuery, template)
    }

    /// A temporal evolution query.
    #[must_use]
    pub fn temporal_query(self, template: &str) -> Request {
        self.typed(Document::TemporalQuery, template)
    }

    /// A local `@context`.
    #[must_use]
    pub fn context(self, template: &str) -> Request {
        self.typed(Document::Context, template)
    }

    /// An array of entity identifiers.
    #[must_use]
    pub fn entity_ids(self, template: &str) -> Request {
        self.typed(Document::EntityIds, template)
    }

    /// JSON sent as written.
    #[must_use]
    pub fn json(self, template: &str) -> Request {
        self.body(Body::Json(template.to_owned()))
    }

    /// Text sent as written.
    #[must_use]
    pub fn text(self, template: &str) -> Request {
        self.body(Body::Text(template.to_owned()))
    }

    /// A payload generated when the request is sent.
    #[must_use]
    pub fn payload(self, payload: Payload) -> Request {
        self.body(Body::Generated(payload))
    }

    /// Sent again up to `count` more times, a second apart, until the answer meets the expectation; never for
    /// less than the delivery deadline.
    #[must_use]
    pub const fn retry(mut self, count: u32) -> Request {
        self.retry = Some(Retry {
            count,
            interval: DEFAULT_RETRY_INTERVAL,
        });
        self
    }

    /// Retried attempts `interval` apart.
    #[must_use]
    pub const fn retry_interval(mut self, interval: Duration) -> Request {
        let count = match self.retry {
            Some(retry) => retry.count,
            None => 0,
        };
        self.retry = Some(Retry { count, interval });
        self
    }

    /// Sent `times` times in a row, each answer checked.
    #[must_use]
    pub const fn repeat(mut self, times: u32) -> Request {
        self.repeat = times;
        self
    }
}
